//! Additive D-120 query payloads preserve old query sessions and bounded page cursors.
use kelvo_proto::{Answer, ErrorCode, Message, Query, decode_message, encode_message};
use kelvo_schema::HostId;
use kelvo_schema::durable_usage::*;
use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(tag = "q", content = "a")]
enum BeforeUsageQuery {
    SpaceStatus,
    #[serde(other)]
    Unknown,
}

#[test]
fn durable_requests_round_trip_and_an_old_query_decoder_refuses_only_the_new_query() {
    let requests = [
        Query::HistoricalUsage(HistoricalUsageRequest {
            host: HostId(uuid::Uuid::nil()),
            from_ms: 0,
            to_ms: 60000,
            by: HistoricalUsageMetric::Network,
            app_names: vec!["Browser".into()],
        }),
        Query::UsageProcesses(UsageProcessRequest {
            host: HostId(uuid::Uuid::nil()),
            from_ms: 0,
            to_ms: 60000,
            app_name: "Browser".into(),
            by: HistoricalUsageMetric::Memory,
            page_size: 100,
            cursor: Some("snapshot-bound-continuation".into()),
        }),
    ];
    for query in requests {
        let body = ciborium::Value::serialized(&query).unwrap();
        // D-117's lenient decoder treats an unreadable new body as Unknown.
        let old = body
            .deserialized::<BeforeUsageQuery>()
            .unwrap_or(BeforeUsageQuery::Unknown);
        assert_eq!(old, BeforeUsageQuery::Unknown);
        let message = Message::Request { id: 17, query };
        assert_eq!(
            decode_message(&encode_message(&message).unwrap()).unwrap(),
            message
        );
    }
    let old_query = ciborium::Value::serialized(&Query::SpaceStatus).unwrap();
    assert_eq!(
        old_query.deserialized::<BeforeUsageQuery>().unwrap(),
        BeforeUsageQuery::SpaceStatus
    );
}

#[test]
fn page_continuation_and_incomplete_evidence_survive_the_wire() {
    let range = UsageHistoryRange {
        requested_from_ms: 0,
        requested_to_ms: 60000,
        spans: vec![UsageHistorySpan {
            from_ms: 10000,
            to_ms: 60000,
            resolution_ms: 10000,
        }],
        coverage: vec![UsageMetricCoverage {
            metric: HistoricalUsageMetric::Cpu,
            coverage: UsageResourceCoverage {
                supported: true,
                measured_ms: 50000,
                complete: false,
                causes: vec![UsageIncompleteCause::UnreadableProcess],
            },
        }],
    };
    let answers = [
        Answer::UsageProcesses(UsageProcessReply {
            range: range.clone(),
            app_name: "Browser".into(),
            processes: vec![HistoricalUsageProcess {
                pid: 42,
                start_time_us: 1_790_000_000_000_000,
                name: "retained worker".into(),
                figures: HistoricalUsageFigures {
                    cpu_avg_pct: Some(12.5),
                    gpu_avg_pct: None,
                    mem_peak_bytes: Some(1 << 40),
                    mem_avg_bytes: Some(1 << 39),
                    energy_j: Some(2.5),
                    cpu_power_w: Some(0.05),
                    network_rx_bytes: None,
                    network_tx_bytes: None,
                    coverage: range.coverage.clone(),
                },
            }],
            network_app_level_only: true,
            next_cursor: Some("next-snapshot-page".into()),
        }),
        Answer::HistoricalUsage(HistoricalUsageReply {
            range,
            by: HistoricalUsageMetric::Network,
            apps: Vec::new(),
            ranking_complete: false,
            network_uncertainty: vec![UsageNetworkUncertainty {
                app_name: None,
                possible_from_ms: 0,
                possible_to_ms: 90000,
                rx_bytes: 100,
                tx_bytes: 10,
                records: 1,
            }],
        }),
        Answer::Error {
            code: ErrorCode::InvalidArgument,
            message: "process history changed; restart expansion".into(),
        },
    ];
    for answer in answers {
        let message = Message::Reply { id: 17, answer };
        assert_eq!(
            decode_message(&encode_message(&message).unwrap()).unwrap(),
            message
        );
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BeforeUsageErrorCode {
    Unsupported,
    #[serde(other)]
    Unknown,
}

#[test]
fn durable_support_is_negotiated_separately_and_new_errors_remain_unknown_to_old_peers() {
    use kelvo_proto::{Offer, USAGE_HISTORY_FEATURE, local_features, negotiate};
    let current = Offer::local(local_features());
    assert!(current.features.contains(USAGE_HISTORY_FEATURE));
    let mut older = current.clone();
    older.features.remove(USAGE_HISTORY_FEATURE);
    let negotiated = negotiate(&current, &older).unwrap();
    assert!(negotiated.features.contains(kelvo_proto::QUERY_FEATURE));
    assert!(!negotiated.features.contains(USAGE_HISTORY_FEATURE));
    assert!(
        negotiate(&current, &current)
            .unwrap()
            .features
            .contains(USAGE_HISTORY_FEATURE)
    );
    for code in [ErrorCode::InvalidArgument, ErrorCode::HistoryUnavailable] {
        let encoded = ciborium::Value::serialized(&code).unwrap();
        assert_eq!(
            encoded.deserialized::<BeforeUsageErrorCode>().unwrap(),
            BeforeUsageErrorCode::Unknown
        );
    }
}
