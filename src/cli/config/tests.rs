use super::*;
use crate::cli::{Cli, Command};
use clap::Parser;

#[test]
fn parses_export_kinds_and_optional_output_without_marking_commands_standalone() {
    for (kind, expected) in [
        ("gateway", DocumentKind::Gateway),
        ("policies", DocumentKind::Policies),
    ] {
        for output in [false, true] {
            let mut args = vec!["stargate", "config", "export", "--kind", kind];
            if output {
                args.extend(["--output", "backup.yaml"]);
            }
            let command = Cli::try_parse_from(args).unwrap().command.unwrap();
            assert!(!command.is_standalone());
            let Command::Config(ConfigArgs {
                command: ConfigCommand::Export(args),
            }) = command
            else {
                panic!("expected config export");
            };
            assert_eq!(args.kind, expected);
            assert_eq!(args.output, output.then(|| PathBuf::from("backup.yaml")));
        }
    }
}

#[test]
fn parses_import_kinds_and_required_file() {
    for kind in ["gateway", "policies"] {
        let command = Cli::try_parse_from([
            "stargate",
            "config",
            "import",
            "--kind",
            kind,
            "--file",
            "input.yaml",
        ])
        .unwrap()
        .command
        .unwrap();
        assert!(!command.is_standalone());
        let Command::Config(ConfigArgs {
            command: ConfigCommand::Import(args),
        }) = command
        else {
            panic!("expected config import");
        };
        assert_eq!(args.file, PathBuf::from("input.yaml"));
        assert_eq!(ConfigKind::from(args.kind).as_str(), kind);
    }
}

#[test]
fn rejects_unknown_kinds_and_missing_required_arguments() {
    for args in [
        vec!["stargate", "config"],
        vec!["stargate", "config", "export"],
        vec!["stargate", "config", "export", "--kind", "unknown"],
        vec!["stargate", "config", "import", "--kind", "gateway"],
        vec!["stargate", "config", "import", "--file", "input.yaml"],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn import_preserves_content_and_audits_both_kinds_without_local_signing_or_tls_material() {
    use crate::data::db::with_test_database;
    use ::db::ent::OutboxEventFilter;
    use gate::cfg::Config;
    use serde_json::{Value, json};
    with_test_database(async {
        let mut raw = serde_json::to_value(Config::default()).unwrap();
        raw["mtls"] = json!({
            "ca_cert_path": "/nonexistent/import-test/ca.pem",
            "client_cert_path": "/nonexistent/import-test/client.pem",
            "client_key_path": "/nonexistent/import-test/key.pem",
        });
        raw["http"] = json!({
            "upstreams": {"secure": {
                "targets": [{"url": "https://upstream.example"}],
                "internal_context": {"audience": "import-test"},
            }},
            "services": {"secure": {"kind": "load_balancer", "upstream": "secure"}},
        });
        let raw: Config = serde_json::from_value(raw).unwrap();
        let content = format!(
            "# Keep the original seed formatting\n{}",
            raw.to_yaml_string().unwrap()
        );
        let saved = import_document(ConfigKind::Gateway, content.clone())
            .await
            .unwrap();
        let ConfigSaveOutcome::Saved(document) = saved else {
            panic!("new gateway document")
        };
        assert_eq!(document.version, 1);
        assert_eq!(document.content, content);
        assert_eq!(document.source, "cli_import");
        assert_eq!(document.created_by, None);
        assert_eq!(
            active_document(ConfigKind::Gateway).await.unwrap().content,
            content
        );
        assert!(matches!(
            import_document(ConfigKind::Gateway, content).await.unwrap(),
            ConfigSaveOutcome::Unchanged(_)
        ));
        let policies = "# @id cli-import\nALLOW user FOR \"imported:READ\";\n";
        assert!(matches!(
            import_document(ConfigKind::Policies, policies.to_string())
                .await
                .unwrap(),
            ConfigSaveOutcome::Saved(_)
        ));
        assert_eq!(
            active_document(ConfigKind::Policies).await.unwrap().content,
            policies
        );
        let events = db::service()
            .query_outbox_events(&OutboxEventFilter::default(), 10, 0)
            .await
            .unwrap();
        assert_eq!(events.len(), 2);
        for row in events {
            let event: Value = serde_json::from_str(&row.payload).unwrap();
            assert_eq!(event["metadata"]["source"], "cli_import");
            assert_eq!(event["actor"]["type"], "system");
            assert_eq!(event["scope"]["kind"], "control_plane");
            assert!(event["request"].is_null());
            assert!(event["after"].get("content").is_none());
        }
    })
    .await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn invalid_imports_do_not_save_and_export_requires_an_active_document() {
    use crate::data::db::with_test_database;
    use ::db::ent::OutboxEventFilter;
    with_test_database(async {
        for kind in [ConfigKind::Gateway, ConfigKind::Policies] {
            assert_eq!(active_document(kind).await.unwrap_err().kind(), io::ErrorKind::NotFound);
        }
        for (kind, content) in [
            (ConfigKind::Gateway, "schema: stargate/v2\nhttp: {}"),
            (ConfigKind::Gateway, "http: {}"),
            (ConfigKind::Policies, "ALLOW user WHEN invalid;"),
            (ConfigKind::Policies, "# @id duplicate\nALLOW user FOR \"one\";\n# @id duplicate\nALLOW user FOR \"two\";"),
        ] {
            assert_eq!(import_document(kind, content.to_string()).await.unwrap_err().kind(), io::ErrorKind::InvalidData);
            assert!(db::get_active_config_document(kind).await.unwrap().is_none());
        }
        assert_eq!(db::service().count_outbox_events(&OutboxEventFilter::default()).await.unwrap(), 0);
    }).await;
}
