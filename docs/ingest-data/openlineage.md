---
title: OpenLineage events with Python
description: A short tutorial describing how to send OpenLineage events to Quickwit with the OpenLineage Python client
tags: [openlineage, lineage, python, ingest-api, integration]
icon_url: /img/tutorials/quickwit-logo.svg
sidebar_position: 6
---

[OpenLineage](https://openlineage.io) is an open standard for collecting lineage metadata about data pipelines. In this tutorial, we will send OpenLineage events describing both the **configuration** and the **execution** of a small ETL pipeline to Quickwit, using the [OpenLineage Python client](https://openlineage.io/docs/client/python) and the Quickwit [ingest API](ingest-api.md).

OpenLineage defines three kinds of events:
- `JobEvent` and `DatasetEvent` carry **static lineage**: the configuration of jobs and datasets (job type, SQL query, source code location, owners, input and output datasets, schemas...).
- `RunEvent` carries **runtime lineage**: the execution of a job. A run emits a `START` event, optionally `RUNNING` events, and ends with a `COMPLETE`, `FAIL`, or `ABORT` event. Run facets describe the execution: nominal time, execution parameters, parent run, processing engine, error message...

All of them share the same structure (`eventTime`, `producer`, `schemaURL`, `job`, `run`, `inputs`, `outputs`), so we store them in a single index.

You will need a [local Quickwit instance](../get-started/installation) up and running and Python 3.10+ to follow this tutorial.

To start Quickwit, run `./quickwit run` in a terminal.

The files used in this tutorial are available in [config/tutorials/openlineage](https://github.com/quickwit-oss/quickwit/tree/main/config/tutorials/openlineage).

## Create the index

The index config declares the fields common to all OpenLineage events. `eventTime` is used as the timestamp field. The job and run facets are open-ended (the spec allows custom facets), so they are mapped as `json` fields and the index uses the `dynamic` mode to keep any other field.

```yaml title="index-config.yaml"
version: 0.8

index_id: openlineage

doc_mapping:
  mode: dynamic
  field_mappings:
    - name: eventTime
      type: datetime
      input_formats: [rfc3339]
      output_format: rfc3339
      fast: true
      fast_precision: milliseconds
    - name: eventType
      type: text
      tokenizer: raw
      fast: true
    - name: producer
      type: text
      tokenizer: raw
    - name: schemaURL
      type: text
      tokenizer: raw
    - name: run
      type: object
      field_mappings:
        - name: runId
          type: text
          tokenizer: raw
          fast: true
        - name: facets
          type: json
          tokenizer: default
    - name: job
      type: object
      field_mappings:
        - name: namespace
          type: text
          tokenizer: raw
          fast: true
        - name: name
          type: text
          tokenizer: raw
          fast: true
        - name: facets
          type: json
          tokenizer: default
    # Only set on `DatasetEvent`.
    - name: dataset
      type: object
      field_mappings:
        - name: namespace
          type: text
          tokenizer: raw
          fast: true
        - name: name
          type: text
          tokenizer: raw
          fast: true
        - name: facets
          type: json
          tokenizer: default
    - name: inputs
      type: array<json>
      tokenizer: raw
    - name: outputs
      type: array<json>
      tokenizer: raw
  timestamp_field: eventTime

search_settings:
  default_search_fields: [job.name, job.namespace, run.facets, job.facets]

indexing_settings:
  commit_timeout_secs: 10
```

```bash
curl -O https://raw.githubusercontent.com/quickwit-oss/quickwit/main/config/tutorials/openlineage/index-config.yaml
./quickwit index create --index-config index-config.yaml
```

## Configure the OpenLineage client

The OpenLineage HTTP transport posts each event as a single-line JSON document to `<url>/<endpoint>`. This is exactly what the Quickwit ingest API expects, so we only need to point the transport to the `openlineage` index ingest endpoint:

```yaml title="openlineage.yml"
transport:
  type: http
  url: http://localhost:7280
  endpoint: api/v1/openlineage/ingest
  timeout: 10
```

The client looks for this file in the path given by the `OPENLINEAGE_CONFIG` environment variable, then in the current directory, then in `~/.openlineage/`. Alternatively, the same configuration can be passed with environment variables:

```bash
export OPENLINEAGE_URL=http://localhost:7280
export OPENLINEAGE_ENDPOINT=api/v1/openlineage/ingest
```

Any tool shipping with an OpenLineage integration (Airflow, Spark, dbt, Flink...) can be configured the same way to send its lineage to Quickwit.

## Send events

The [send_openlineage_events.py](https://github.com/quickwit-oss/quickwit/tree/main/config/tutorials/openlineage/send_openlineage_events.py) script simulates a `daily_orders` pipeline made of two tasks, `daily_orders.aggregate_revenue` and `daily_orders.build_report`, reading from and writing to PostgreSQL tables. It emits:
- the **configuration** of the pipeline and of its tasks: one `JobEvent` per job, with the `jobType`, `documentation`, `ownership`, `sourceCodeLocation`, and `sql` job facets, and the input and output datasets with their `schema` facet.
- the **execution** of the pipeline for each of the last 30 days: `START` and `COMPLETE` or `FAIL` `RunEvent`s for the pipeline and each of its tasks, with the `nominalTime`, `executionParameters`, `parent`, `processingEngine`, and `errorMessage` run facets, and the `outputStatistics` facet on output datasets.

Here is how a task run is reported with the Python client:

```python
from openlineage.client import OpenLineageClient
from openlineage.client.event_v2 import Job, Run, RunEvent, RunState
from openlineage.client.facet_v2 import execution_parameters_run, parent_run
from openlineage.client.uuid import generate_new_uuid

# Reads `openlineage.yml` or the `OPENLINEAGE_*` environment variables.
client = OpenLineageClient()

run = Run(
    runId=str(generate_new_uuid()),
    facets={
        "parent": parent_run.ParentRunFacet(
            run=parent_run.Run(runId=pipeline_run_id),
            job=parent_run.Job(namespace="quickwit-tutorial", name="daily_orders"),
        ),
        "executionParameters": execution_parameters_run.ExecutionParametersRunFacet(
            parameters=[execution_parameters_run.ExecutionParameter(key="day", value="2026-10-20")]
        ),
    },
)
job = Job(namespace="quickwit-tutorial", name="daily_orders.aggregate_revenue")
client.emit(RunEvent(eventType=RunState.START, eventTime=start_time, run=run, job=job, producer=PRODUCER))
```

Download the script and its client configuration, install the client, and run it:

```bash
curl -O https://raw.githubusercontent.com/quickwit-oss/quickwit/main/config/tutorials/openlineage/send_openlineage_events.py
curl -O https://raw.githubusercontent.com/quickwit-oss/quickwit/main/config/tutorials/openlineage/openlineage.yml
pip install openlineage-python
python send_openlineage_events.py
```

```
Sent 3 configuration events.
Sent 175 events in total (30 pipeline runs).
```

The number of events varies from one execution to another as task runs fail randomly (`--failure-rate`, 10% by default). Use `--days` to change the number of simulated pipeline runs and `--seed` to get a reproducible output.

Events are committed within 10 seconds (`commit_timeout_secs`).

## Search events

Let's start with the configuration of the tasks reading the `public.orders` table:

```bash
curl "http://localhost:7280/api/v1/openlineage/search?query=schemaURL:\"https://openlineage.io/spec/2-0-2/OpenLineage.json%23/\$defs/JobEvent\"%20AND%20inputs.name:public.orders"
```

Then the failed runs and their error message:

```bash
curl "http://localhost:7280/api/v1/openlineage/search?query=eventType:FAIL%20AND%20run.facets.errorMessage.message:deadlock&sort_by=-eventTime"
```

All the events of a pipeline run can be retrieved from the run ID of the pipeline, as task runs reference it in their `parent` facet:

```bash
curl "http://localhost:7280/api/v1/openlineage/search?query=run.runId:<run-id>%20OR%20run.facets.parent.run.runId:<run-id>&sort_by=eventTime"
```

Finally, aggregations give an overview of the executions, for instance the number of runs per job and final state:

```bash
curl -XPOST -H 'Content-Type: application/json' http://localhost:7280/api/v1/openlineage/search --data '{
  "query": "eventType:COMPLETE OR eventType:FAIL",
  "max_hits": 0,
  "aggs": {
    "jobs": {
      "terms": {"field": "job.name"},
      "aggs": {"states": {"terms": {"field": "eventType"}}}
    }
  }
}'
```

## Further reading

- [OpenLineage object model](https://openlineage.io/docs/spec/object-model)
- [OpenLineage facets](https://openlineage.io/docs/spec/facets/)
- [Ingest API](ingest-api.md)
- [Index configuration](../configuration/index-config.md)
