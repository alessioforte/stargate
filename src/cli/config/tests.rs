use super::*;
use crate::cli::{Cli, Command};
use clap::Parser;

#[test]
fn invalid_import_errors_include_gateway_paths_and_policy_lines() {
    let mut config = gate::cfg::Config::default();
    config.ingress.limit = "unknown-limit".into();
    let error =
        validate_document(ConfigKind::Gateway, &config.to_yaml_string().unwrap()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("ingress.limit"));
    let error = validate_document(ConfigKind::Policies, "\nALLOW user WHEN invalid;").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("line 2:"));
}

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
            assert_eq!(args.version, None);
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

#[test]
fn parses_version_listing_with_a_positive_limit_and_historical_export() {
    for kind in ["gateway", "policies"] {
        for limit in [None, Some("3")] {
            let mut command = vec!["stargate", "config", "versions", "--kind", kind];
            if let Some(limit) = limit {
                command.extend(["--limit", limit]);
            }
            let parsed = Cli::try_parse_from(command).unwrap().command.unwrap();
            assert!(!parsed.is_standalone());
            let Command::Config(ConfigArgs {
                command: ConfigCommand::Versions(args),
            }) = parsed
            else {
                panic!("expected config versions")
            };
            assert_eq!(ConfigKind::from(args.kind).as_str(), kind);
            assert_eq!(
                args.limit,
                limit.map_or(20, |value| value.parse::<i64>().unwrap())
            );
        }
        let parsed = Cli::try_parse_from([
            "stargate",
            "config",
            "export",
            "--kind",
            kind,
            "--version",
            "41",
            "--output",
            "old.yaml",
        ])
        .unwrap()
        .command
        .unwrap();
        let Command::Config(ConfigArgs {
            command: ConfigCommand::Export(args),
        }) = parsed
        else {
            panic!("expected config export")
        };
        assert_eq!(args.version, Some(41));
        assert_eq!(args.output, Some(PathBuf::from("old.yaml")));
        assert_eq!(ConfigKind::from(args.kind).as_str(), kind);
    }
    for (command, flag) in [("versions", "--limit"), ("export", "--version")] {
        for invalid in ["0", "-1", "bad", "9223372036854775808"] {
            assert!(
                Cli::try_parse_from([
                    "stargate", "config", command, "--kind", "gateway", flag, invalid
                ])
                .is_err()
            );
        }
    }
    assert!(Cli::try_parse_from(["stargate", "config", "versions"]).is_err());
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn historical_export_preserves_bytes_and_import_restores_as_a_new_audited_version() {
    use ::db::ent::OutboxEventFilter;
    crate::data::db::with_test_database(async {
        let first = format!("# Original café configuration\r\n{}\n", gate::cfg::Config::default().to_yaml_string().unwrap());
        let mut changed = gate::cfg::Config::default();
        changed.ingress.timeout = "300ms".into();
        let second = changed.to_yaml_string().unwrap();
        import_document(ConfigKind::Gateway, first.clone()).await.unwrap();
        import_document(ConfigKind::Gateway, second.clone()).await.unwrap();
        import_document(ConfigKind::Policies, String::new()).await.unwrap();
        let path = std::env::temp_dir().join(format!("stargate-version-export-{}.yaml", ulid::Ulid::generate()));
        export_document(ExportArgs {kind: DocumentKind::Gateway, version: Some(1), output: Some(path.clone())}).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), first.as_bytes());
        assert_eq!(active_document(ConfigKind::Gateway).await.unwrap().content, second);
        let listed = db::list_config_versions(ConfigKind::Gateway, 20, 0).await.unwrap();
        assert_eq!(listed.iter().map(|entry| entry.version).collect::<Vec<_>>(), vec![2, 1]);
        let mut table = Vec::new();
        write_versions(&listed, &mut table).unwrap();
        let table = String::from_utf8(table).unwrap();
        let rows: Vec<_> = table.lines().collect();
        assert_eq!(rows.len(), 3);
        assert!(rows[0].contains("SIZE_BYTES"));
        for (index, entry) in listed.iter().enumerate() {
            let columns: Vec<_> = rows[index+1].split_whitespace().collect();
            assert_eq!(columns[0], if index == 0 {"active"} else {"historical"});
            assert_eq!(columns[1], entry.version.to_string());
            assert_eq!(columns[2], entry.revision);
            assert_eq!(columns[3], "cli_import");
            assert_eq!(columns[4], "-");
            assert_eq!(chrono::DateTime::parse_from_rfc3339(columns[5]).unwrap(), entry.created_at);
            assert_eq!(columns[6], entry.size_bytes.to_string());
        }
        let outcome = import_document(ConfigKind::Gateway, std::fs::read_to_string(&path).unwrap()).await.unwrap();
        assert!(matches!(outcome, ConfigSaveOutcome::Saved(document) if document.version == 3 && document.content == first));
        assert!(matches!(import_document(ConfigKind::Gateway, first.clone()).await.unwrap(), ConfigSaveOutcome::Unchanged(document) if document.version == 3));
        let events = db::service().query_outbox_events(&OutboxEventFilter::default(), 10, 0).await.unwrap();
        assert_eq!(events.len(), 4);
        let restored = events.iter().map(|row| serde_json::from_str::<serde_json::Value>(&row.payload).unwrap()).find(|event| event["after"]["version"] == 3).unwrap();
        assert_eq!(restored["metadata"]["source"], "cli_import");
        assert_eq!(restored["actor"]["type"], "system");
        assert!(restored["after"].get("content").is_none());
        export_document(ExportArgs {kind: DocumentKind::Gateway, version: None, output: Some(path.clone())}).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), first.as_bytes());
        std::fs::remove_file(&path).unwrap();
        for kind in [DocumentKind::Gateway, DocumentKind::Policies] {
            let error = export_document(ExportArgs {kind, version: Some(41), output: Some(path.clone())}).await.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
            assert_eq!(error.to_string(), format!("no {} document version 41 in the database", ConfigKind::from(kind).as_str()));
            assert!(!path.exists());
        }
    }).await;
}
