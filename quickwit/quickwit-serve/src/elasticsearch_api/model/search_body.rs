// Copyright 2021-Present Datadog, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::BTreeSet;
use std::fmt;

use quickwit_proto::search::{SortMissing, SortOrder};
use quickwit_query::{ElasticQueryDsl, OneFieldMap};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::ElasticDateFormat;
use crate::elasticsearch_api::TrackTotalHits;
use crate::elasticsearch_api::model::{SortField, default_elasticsearch_sort_order};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
enum FieldSortParamsForDeser {
    // we can't just use FieldSortParams or we get infinite recursion on deser
    Object {
        order: Option<SortOrder>,
        format: Option<ElasticDateFormat>,
        // Kept as a raw JSON value and validated in `TryFrom`, so that unsupported values are
        // rejected instead of making this variant silently fail to match.
        missing: Option<serde_json::Value>,
    },
    String(SortOrder),
}

/// Parses the Elasticsearch `missing` sort option.
///
/// Only `_first` and `_last` are supported. Elasticsearch also accepts a custom value to use
/// for the documents without a value, which Quickwit does not support.
fn parse_sort_missing(missing: serde_json::Value) -> Result<SortMissing, String> {
    match missing.as_str() {
        Some("_last") => Ok(SortMissing::Last),
        Some("_first") => Ok(SortMissing::First),
        _ => Err(format!(
            "unsupported sort `missing` value `{missing}`: only `_first` and `_last` are supported"
        )),
    }
}

impl TryFrom<FieldSortParamsForDeser> for FieldSortParams {
    type Error = String;

    fn try_from(for_deser: FieldSortParamsForDeser) -> Result<FieldSortParams, String> {
        match for_deser {
            FieldSortParamsForDeser::Object {
                order,
                format: date_format,
                missing,
            } => {
                let missing = match missing {
                    Some(missing) => parse_sort_missing(missing)?,
                    None => SortMissing::Last,
                };
                Ok(FieldSortParams {
                    order,
                    date_format,
                    missing,
                })
            }
            FieldSortParamsForDeser::String(order) => Ok(FieldSortParams {
                order: Some(order),
                date_format: None,
                missing: SortMissing::Last,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FieldSortParamsForDeser")]
#[serde(deny_unknown_fields)]
struct FieldSortParams {
    #[serde(default)]
    pub order: Option<SortOrder>,
    #[serde(default)]
    #[serde(rename = "format")]
    pub date_format: Option<ElasticDateFormat>,
    #[serde(default)]
    pub missing: SortMissing,
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SearchBody {
    #[serde(default)]
    pub from: Option<u64>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub query: Option<ElasticQueryDsl>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_field_sorts")]
    pub sort: Option<Vec<SortField>>,
    #[serde(default, alias = "aggregations")]
    pub aggs: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub track_total_hits: Option<TrackTotalHits>,
    #[serde(default)]
    pub stored_fields: Option<BTreeSet<String>>,
    #[serde(default)]
    pub search_after: Vec<serde_json::Value>,

    // Ignored values, only here for compatibility with OpenSearch Dashboards.
    #[serde(default)]
    pub _source: serde::de::IgnoredAny,
    #[serde(default)]
    pub docvalue_fields: serde::de::IgnoredAny,
    #[serde(default)]
    pub script_fields: serde::de::IgnoredAny,
    #[serde(default)]
    pub highlight: serde::de::IgnoredAny,
    #[serde(default)]
    pub version: serde::de::IgnoredAny,
}

struct FieldSortVecVisitor;

#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrMapFieldSort {
    FieldNameOnly(String),
    Sort(OneFieldMap<FieldSortParams>),
}

impl From<StringOrMapFieldSort> for SortField {
    fn from(string_or_map_field_sort: StringOrMapFieldSort) -> Self {
        match string_or_map_field_sort {
            StringOrMapFieldSort::FieldNameOnly(field_name) => {
                let order = default_elasticsearch_sort_order(&field_name);
                SortField {
                    field: field_name,
                    order,
                    date_format: None,
                    missing: SortMissing::Last,
                }
            }
            StringOrMapFieldSort::Sort(sort) => {
                let order = sort
                    .value
                    .order
                    .unwrap_or_else(|| default_elasticsearch_sort_order(&sort.field));
                SortField {
                    field: sort.field,
                    order,
                    date_format: sort.value.date_format,
                    missing: sort.value.missing,
                }
            }
        }
    }
}

impl<'de> Visitor<'de> for FieldSortVecVisitor {
    type Value = Vec<SortField>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("A string, array, or object containing the sort fields.")
    }

    fn visit_str<E>(self, field_name: &str) -> Result<Vec<SortField>, E>
    where E: serde::de::Error {
        let order = default_elasticsearch_sort_order(field_name);
        Ok(vec![SortField {
            field: field_name.to_string(),
            order,
            date_format: None,
            missing: SortMissing::Last,
        }])
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Vec<SortField>, A::Error>
    where A: serde::de::SeqAccess<'de> {
        let mut sort_fields: Vec<SortField> = Vec::new();
        while let Some(field_sort) = seq.next_element::<StringOrMapFieldSort>()? {
            sort_fields.push(field_sort.into());
        }
        Ok(sort_fields)
    }

    fn visit_map<M>(self, mut map: M) -> Result<Vec<SortField>, M::Error>
    where M: MapAccess<'de> {
        let mut sort_fields: Vec<SortField> = Vec::new();
        while let Some((field_sort_key, field_sort_params)) =
            map.next_entry::<String, FieldSortParams>()?
        {
            let sort_order = field_sort_params
                .order
                .unwrap_or_else(|| default_elasticsearch_sort_order(&field_sort_key));
            sort_fields.push(SortField {
                field: field_sort_key,
                order: sort_order,
                date_format: field_sort_params.date_format,
                missing: field_sort_params.missing,
            });
        }
        Ok(sort_fields)
    }
}

/// ES accepts structs to describe the sort field.
/// In that case the order of apparition in the JSON object matters.
fn deserialize_field_sorts<'de, D>(deserializer: D) -> Result<Option<Vec<SortField>>, D::Error>
where D: Deserializer<'de> {
    deserializer.deserialize_any(FieldSortVecVisitor).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_field_array() {
        let json = r#"
        {
            "sort": [
                { "timestamp": { "order": "desc", "format": "epoch_nanos_int" } },
                { "uid": { "order": "asc" } },
                { "my_field": "asc" },
                { "hello": {}},
                { "_score": {}}
            ]
        }
        "#;
        let search_body: SearchBody = serde_json::from_str(json).unwrap();
        let sort_fields = search_body.sort.unwrap();
        assert_eq!(sort_fields.len(), 5);
        assert_eq!(sort_fields[0].field, "timestamp");
        assert_eq!(sort_fields[0].order, SortOrder::Desc);
        assert_eq!(
            sort_fields[0].date_format,
            Some(ElasticDateFormat::EpochNanosInt)
        );
        assert_eq!(sort_fields[1].field, "uid");
        assert_eq!(sort_fields[1].order, SortOrder::Asc);
        assert_eq!(sort_fields[1].date_format, None);
        assert_eq!(sort_fields[2].field, "my_field");
        assert_eq!(sort_fields[2].order, SortOrder::Asc);
        assert_eq!(sort_fields[2].date_format, None);
        assert_eq!(sort_fields[3].field, "hello");
        assert_eq!(sort_fields[3].order, SortOrder::Asc);
        assert_eq!(sort_fields[3].date_format, None);
        assert_eq!(sort_fields[4].field, "_score");
        assert_eq!(sort_fields[4].order, SortOrder::Desc);
        assert_eq!(sort_fields[4].date_format, None);
    }

    #[test]
    fn test_sort_field_missing() {
        let json = r#"
        {
            "sort": [
                { "rank": { "order": "desc", "missing": "_first" } },
                { "uid": { "missing": "_last" } }
            ]
        }
        "#;
        let search_body: SearchBody = serde_json::from_str(json).unwrap();
        let sort_fields = search_body.sort.unwrap();
        assert_eq!(sort_fields.len(), 2);
        assert_eq!(sort_fields[0].field, "rank");
        assert_eq!(sort_fields[0].order, SortOrder::Desc);
        assert_eq!(sort_fields[0].missing, SortMissing::First);
        assert_eq!(sort_fields[1].field, "uid");
        assert_eq!(sort_fields[1].order, SortOrder::Asc);
        assert_eq!(sort_fields[1].missing, SortMissing::Last);

        let json = r#"
        {
            "sort": {
                "rank": { "order": "asc", "missing": "_first" },
                "uid": "desc"
            }
        }
        "#;
        let search_body: SearchBody = serde_json::from_str(json).unwrap();
        let sort_fields = search_body.sort.unwrap();
        assert_eq!(sort_fields.len(), 2);
        assert_eq!(sort_fields[0].missing, SortMissing::First);
        assert_eq!(sort_fields[1].missing, SortMissing::Last);

        let search_body: SearchBody = serde_json::from_str(r#"{ "sort": ["rank"] }"#).unwrap();
        assert_eq!(search_body.sort.unwrap()[0].missing, SortMissing::Last);
    }

    #[test]
    fn test_sort_field_missing_custom_value_unsupported() {
        for missing in [r#""foo""#, "0"] {
            let json_obj = format!(r#"{{ "sort": {{ "rank": {{ "missing": {missing} }} }} }}"#);
            let error_msg = serde_json::from_str::<SearchBody>(&json_obj)
                .unwrap_err()
                .to_string();
            assert!(
                error_msg.contains("only `_first` and `_last` are supported"),
                "{error_msg}"
            );
            let json_array = format!(r#"{{ "sort": [{{ "rank": {{ "missing": {missing} }} }}] }}"#);
            serde_json::from_str::<SearchBody>(&json_array).unwrap_err();
        }
    }

    #[test]
    fn test_sort_field_obj() {
        let json = r#"
        {
            "sort": {
                "timestamp": { "order": "desc" },
                "uid": { "order": "asc" }
            }
        }
        "#;
        let search_body: SearchBody = serde_json::from_str(json).unwrap();
        let field_sorts = search_body.sort.unwrap();
        assert_eq!(field_sorts.len(), 2);
        assert_eq!(field_sorts[0].field, "timestamp");
        assert_eq!(field_sorts[0].order, SortOrder::Desc);
        assert_eq!(field_sorts[1].field, "uid");
        assert_eq!(field_sorts[1].order, SortOrder::Asc);
    }

    #[test]
    fn test_sort_field_str() {
        let json = r#"
        {
            "sort": "timestamp"
        }
        "#;
        let search_body: SearchBody = serde_json::from_str(json).unwrap();
        let field_sorts = search_body.sort.unwrap();
        assert_eq!(field_sorts.len(), 1);
        assert_eq!(field_sorts[0].field, "timestamp");
        assert_eq!(field_sorts[0].order, SortOrder::Asc);
    }

    #[test]
    fn test_sort_default_orders() {
        let json = r#"
        {
            "sort": [
                "timestamp",
                "uid",
                "_score",
                "_doc"
            ]
        }
        "#;
        let search_body: SearchBody = serde_json::from_str(json).unwrap();
        let field_sorts = search_body.sort.unwrap();
        assert_eq!(field_sorts.len(), 4);
        assert_eq!(field_sorts[0].field, "timestamp");
        assert_eq!(field_sorts[0].order, SortOrder::Asc);
        assert_eq!(field_sorts[1].field, "uid");
        assert_eq!(field_sorts[1].order, SortOrder::Asc);
        assert_eq!(field_sorts[2].field, "_score");
        assert_eq!(field_sorts[2].order, SortOrder::Desc);
        assert_eq!(field_sorts[3].field, "_doc");
        assert_eq!(field_sorts[3].order, SortOrder::Asc);
    }

    #[test]
    fn test_unknown_field_behaviour() {
        let json = r#"
            {
                "term": {
                    "actor.id": {
                        "value": "95077794"
                     }
                }
            }
        "#;

        let search_body = serde_json::from_str::<SearchBody>(json);
        let error_msg = search_body.unwrap_err().to_string();
        assert!(error_msg.contains("unknown field `term`"));
        assert!(error_msg.contains("expected one of "));
    }
}
