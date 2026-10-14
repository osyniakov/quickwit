// Throwaway probe for quickwit-oss/quickwit#6892. Not meant to be committed.
//
// Runs many distinct, time-bounded, selective queries against a long-lived searcher context and
// measures heap growth with the predicate cache at its default capacity vs. disabled (0).

use std::alloc::{GlobalAlloc, Layout, System};
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};

use bytesize::ByteSize;
use quickwit_config::SearcherConfig;
use quickwit_indexing::TestSandbox;
use quickwit_proto::search::SearchRequest;
use quickwit_query::query_ast::qast_json_helper;
use quickwit_proto::types::NodeId;
use serde_json::json;

use crate::search_job_placer::SearchJobPlacer;
use crate::SearchServiceClient;
use crate::service::SearcherContext;
use crate::SearchServiceImpl;
use crate::{ClusterClient, SearcherNode, SearcherPool, root_search};

struct CountingAllocator;

static LIVE_HEAP_BYTES: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE_HEAP_BYTES.fetch_add(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_HEAP_BYTES.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE_HEAP_BYTES.fetch_add(new_size as isize - layout.size() as isize, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const NUM_SPLITS: usize = 40;
const DOCS_PER_SPLIT: usize = 500;

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

async fn run_probe(test_sandbox: &TestSandbox, predicate_cache_capacity: ByteSize, num_queries: usize) -> isize {
    let mut searcher_config = SearcherConfig::default();
    searcher_config.predicate_cache = predicate_cache_capacity.into();
    let searcher_context = Arc::new(SearcherContext::new_without_invoker(searcher_config, None));

    let socket_addr = SocketAddr::new(Ipv4Addr::new(127, 0, 0, 1).into(), 7280u16);
    let searcher_pool = SearcherPool::default();
    let cluster_client = ClusterClient::new(SearchJobPlacer::new(searcher_pool.clone()));
    let search_service = Arc::new(SearchServiceImpl::new(
        test_sandbox.metastore(),
        test_sandbox.storage_resolver(),
        cluster_client.clone(),
        searcher_context.clone(),
    ));
    searcher_pool.insert(
        socket_addr,
        SearcherNode {
            node_id: NodeId::from_str("probe-node"),
            client: SearchServiceClient::from_service(search_service, socket_addr),
        },
    );
    let metastore = test_sandbox.metastore();

    let search = |query_id: usize| {
        let request = SearchRequest {
            index_id_patterns: vec![test_sandbox.index_uid().index_id.to_string()],
            // Selective lookups, like trace-id or request-id searches: zero hits per split.
            query_ast: qast_json_helper(&format!("trace_id:missing{query_id}"), &["body"]),
            start_timestamp: Some(1_000),
            end_timestamp: Some(1_000_000_000),
            max_hits: 10,
            ..Default::default()
        };
        root_search(&searcher_context, request, &metastore, &cluster_client)
    };

    // Warm the footer cache and other one-off allocations so they don't count.
    for query_id in 0..50 {
        search(usize::MAX - query_id).await.unwrap();
    }
    let heap_before = LIVE_HEAP_BYTES.load(Ordering::Relaxed);
    for query_id in 0..num_queries {
        let response = search(query_id).await.unwrap();
        assert_eq!(response.num_hits, 0);
    }
    let heap_after = LIVE_HEAP_BYTES.load(Ordering::Relaxed);
    heap_after - heap_before
}

#[tokio::test]
async fn probe_predicate_cache_memory() -> anyhow::Result<()> {
    let num_queries = env_usize("PROBE_NUM_QUERIES", 2_000);
    let doc_mapping_yaml = r#"
            field_mappings:
              - name: body
                type: text
              - name: trace_id
                type: text
                tokenizer: raw
              - name: ts
                type: datetime
                input_formats: ["unix_timestamp"]
                fast: true
            timestamp_field: ts
            mode: lenient
        "#;
    let test_sandbox =
        TestSandbox::create("probe-6892", doc_mapping_yaml, "{}", &["body"]).await?;
    for split_ord in 0..NUM_SPLITS {
        let docs = (0..DOCS_PER_SPLIT).map(|doc_ord| {
            json!({
                "body": format!("log line {doc_ord}"),
                "trace_id": format!("t{split_ord}-{doc_ord}"),
                "ts": 10_000 + split_ord * 1_000 + doc_ord,
            })
        });
        test_sandbox.add_documents(docs).await?;
    }
    let num_entries = num_queries * NUM_SPLITS;
    // Accounted payload of one entry: 32-byte segment id + 4-byte empty HitSet.
    let accounted_bytes = num_entries * 36;

    let order = std::env::var("PROBE_ORDER").unwrap_or_else(|_| "default-first".to_string());
    let capacities: Vec<(&str, ByteSize)> = if order == "zero-first" {
        vec![("predicate_cache=0", ByteSize::b(0)), ("predicate_cache=256MB (default)", ByteSize::mb(256))]
    } else {
        vec![("predicate_cache=256MB (default)", ByteSize::mb(256)), ("predicate_cache=0", ByteSize::b(0))]
    };
    let mut results = Vec::new();
    for (label, capacity) in capacities {
        let heap_growth = run_probe(&test_sandbox, capacity, num_queries).await;
        results.push((label, heap_growth));
    }
    println!(
        "PROBE queries={num_queries} splits={NUM_SPLITS} max_predicate_entries={num_entries} \
         accounted_payload_if_all_cached={:.1} MiB",
        accounted_bytes as f64 / 1048576.0
    );
    for (label, heap_growth) in &results {
        println!(
            "PROBE {label:32} live_heap_growth={:8.1} MiB  per_entry={:6.1} B",
            *heap_growth as f64 / 1048576.0,
            *heap_growth as f64 / num_entries as f64
        );
    }
    test_sandbox.assert_quit().await;
    Ok(())
}
