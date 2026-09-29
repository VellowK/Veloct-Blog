//! Reading and writing the AI provider settings.
//!
//! `docs/spec/01-data-model.md` §3 fixes the shape: providers, the models discovered from each, the
//! per-feature mapping and exactly ONE global fallback. `handoff.md` §11 asks for the mapping and §11.4
//! for the fallback, and §12 puts every credential behind a `credential_ref` HANDLE.
//!
//! That last point is why [`AiProviderRow`] has `credential_ref: Option<String>` and no field for a
//! secret. There is nowhere to put one, so a caller cannot accidentally persist a key here; the credential
//! itself lives in the OS credential store (§12.2) and this row names it.

use rusqlite::{Connection, OptionalExtension};

use veloct_core::{Result, VeloctError};

/// One configured provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiProviderRow {
    pub id: String,
    pub name: String,
    /// `openai`, `anthropic` or `ollama` (`handoff.md` §11.2).
    pub protocol: String,
    pub base_url: String,
    /// The handle for the OS credential store, NEVER the credential (§12.2).
    pub credential_ref: Option<String>,
    pub enabled: bool,
    pub created_at: String,
}

impl AiProviderRow {
    /// Whether the provider is usable as configured.
    pub fn is_usable(&self) -> bool {
        self.enabled && !self.base_url.trim().is_empty()
    }
}

/// One model offered by a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiModelRow {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: Option<String>,
    /// Where the model came from: discovered from the provider's list, or added by hand.
    pub source: String,
    pub context_window: Option<i64>,
    pub last_seen_at: Option<String>,
}

impl AiModelRow {
    /// The name to show, falling back to the model id.
    pub fn label(&self) -> &str {
        match &self.display_name {
            Some(name) if !name.trim().is_empty() => name,
            _ => &self.model_id,
        }
    }
}

/// The per-feature model mapping (§11.3).
///
/// A feature with no row, or with `enabled` false, is NOT configured: §11.3 requires the user to choose a
/// model per feature rather than have one silently inherited, and an unconfigured feature reports that
/// rather than guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiFeatureRow {
    pub id: String,
    pub label: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub enabled: bool,
}

impl AiFeatureRow {
    pub fn is_configured(&self) -> bool {
        self.enabled && self.provider_id.is_some() && self.model_id.is_some()
    }
}

/// The single global fallback (§11.4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AiFallbackRow {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
}

impl AiFallbackRow {
    pub fn is_configured(&self) -> bool {
        self.provider_id.is_some() && self.model_id.is_some()
    }
}

/// Everything the AI subsystem needs to build a service.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AiSettings {
    pub providers: Vec<AiProviderRow>,
    pub models: Vec<AiModelRow>,
    pub features: Vec<AiFeatureRow>,
    pub fallback: AiFallbackRow,
}

impl AiSettings {
    /// Whether any provider is configured at all.
    ///
    /// The page creator reports a clear, actionable message when this is false rather than failing deep
    /// inside a call, because "no provider is configured" is the first thing a new project hits.
    pub fn has_any_provider(&self) -> bool {
        self.providers.iter().any(AiProviderRow::is_usable)
    }

    /// The models offered by a provider.
    pub fn models_of(&self, provider_id: &str) -> Vec<&AiModelRow> {
        self.models
            .iter()
            .filter(|model| model.provider_id == provider_id)
            .collect()
    }

    /// The row for a feature, by its id.
    pub fn feature(&self, id: &str) -> Option<&AiFeatureRow> {
        self.features.iter().find(|feature| feature.id == id)
    }
}

/// Read the whole AI configuration.
pub fn read_settings(connection: &Connection) -> Result<AiSettings> {
    Ok(AiSettings {
        providers: read_providers(connection)?,
        models: read_models(connection)?,
        features: read_features(connection)?,
        fallback: read_fallback(connection)?,
    })
}

fn db_error(context: &str, error: rusqlite::Error) -> VeloctError {
    VeloctError::Database(format!("{context}: {error}"))
}

pub fn read_providers(connection: &Connection) -> Result<Vec<AiProviderRow>> {
    let mut statement = connection
        .prepare(
            "SELECT id, name, protocol, base_url, credential_ref, enabled, created_at
             FROM ai_providers ORDER BY name COLLATE NOCASE, id",
        )
        .map_err(|error| db_error("could not read the AI providers", error))?;

    let rows = statement
        .query_map([], |row| {
            Ok(AiProviderRow {
                id: row.get(0)?,
                name: row.get(1)?,
                protocol: row.get(2)?,
                base_url: row.get(3)?,
                credential_ref: row.get(4)?,
                enabled: row.get::<_, i64>(5)? != 0,
                created_at: row.get(6)?,
            })
        })
        .map_err(|error| db_error("could not read the AI providers", error))?;

    let mut providers = Vec::new();
    for row in rows {
        providers.push(row.map_err(|error| db_error("could not read an AI provider", error))?);
    }
    Ok(providers)
}

pub fn read_models(connection: &Connection) -> Result<Vec<AiModelRow>> {
    let mut statement = connection
        .prepare(
            "SELECT id, provider_id, model_id, display_name, source, context_window, last_seen_at
             FROM ai_models ORDER BY provider_id, model_id",
        )
        .map_err(|error| db_error("could not read the AI models", error))?;

    let rows = statement
        .query_map([], |row| {
            Ok(AiModelRow {
                id: row.get(0)?,
                provider_id: row.get(1)?,
                model_id: row.get(2)?,
                display_name: row.get(3)?,
                source: row.get(4)?,
                context_window: row.get(5)?,
                last_seen_at: row.get(6)?,
            })
        })
        .map_err(|error| db_error("could not read the AI models", error))?;

    let mut models = Vec::new();
    for row in rows {
        models.push(row.map_err(|error| db_error("could not read an AI model", error))?);
    }
    Ok(models)
}

pub fn read_features(connection: &Connection) -> Result<Vec<AiFeatureRow>> {
    let mut statement = connection
        .prepare("SELECT id, label, provider_id, model_id, enabled FROM ai_features ORDER BY id")
        .map_err(|error| db_error("could not read the AI features", error))?;

    let rows = statement
        .query_map([], |row| {
            Ok(AiFeatureRow {
                id: row.get(0)?,
                label: row.get(1)?,
                provider_id: row.get(2)?,
                model_id: row.get(3)?,
                enabled: row.get::<_, i64>(4)? != 0,
            })
        })
        .map_err(|error| db_error("could not read the AI features", error))?;

    let mut features = Vec::new();
    for row in rows {
        features.push(row.map_err(|error| db_error("could not read an AI feature", error))?);
    }
    Ok(features)
}

pub fn read_fallback(connection: &Connection) -> Result<AiFallbackRow> {
    let row = connection
        .query_row(
            "SELECT provider_id, model_id FROM ai_fallback WHERE id = 1",
            [],
            |row| {
                Ok(AiFallbackRow {
                    provider_id: row.get(0)?,
                    model_id: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|error| db_error("could not read the AI fallback", error))?;
    Ok(row.unwrap_or_default())
}

/// Insert or replace a provider.
///
/// `credential_ref` is a handle, and there is deliberately no parameter for a secret value: §12.2 keeps
/// credentials in the OS store, and a signature that accepted one would make it easy to write a key into
/// the database where it would be backed up and shipped.
pub fn upsert_provider(connection: &Connection, provider: &AiProviderRow) -> Result<()> {
    connection
        .execute(
            "INSERT INTO ai_providers (id, name, protocol, base_url, credential_ref, enabled, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               protocol = excluded.protocol,
               base_url = excluded.base_url,
               credential_ref = excluded.credential_ref,
               enabled = excluded.enabled",
            rusqlite::params![
                provider.id,
                provider.name,
                provider.protocol,
                provider.base_url,
                provider.credential_ref,
                if provider.enabled { 1 } else { 0 },
                provider.created_at,
            ],
        )
        .map_err(|error| db_error("could not write the AI provider", error))?;
    Ok(())
}

/// Remove a provider and everything that referenced it.
pub fn delete_provider(connection: &Connection, id: &str) -> Result<()> {
    connection
        .execute("DELETE FROM ai_providers WHERE id = ?1", [id])
        .map_err(|error| db_error("could not delete the AI provider", error))?;
    // The foreign keys cascade, but the fallback and the features are set to NULL by their own clauses, so
    // an unconfigured feature is left unconfigured rather than pointing at a provider that is gone.
    Ok(())
}

/// Replace the discovered models for a provider.
///
/// A replace rather than a merge: a model the provider no longer offers should disappear, and a merge would
/// leave it selectable forever.
pub fn replace_models(
    connection: &mut Connection,
    provider_id: &str,
    models: &[AiModelRow],
) -> Result<()> {
    let transaction = connection
        .transaction()
        .map_err(|error| db_error("could not start the model refresh", error))?;

    transaction
        .execute("DELETE FROM ai_models WHERE provider_id = ?1", [provider_id])
        .map_err(|error| db_error("could not clear the AI models", error))?;

    for model in models {
        transaction
            .execute(
                "INSERT INTO ai_models
                   (id, provider_id, model_id, display_name, source, context_window, last_seen_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    model.id,
                    provider_id,
                    model.model_id,
                    model.display_name,
                    model.source,
                    model.context_window,
                    model.last_seen_at,
                ],
            )
            .map_err(|error| db_error("could not write an AI model", error))?;
    }

    transaction
        .commit()
        .map_err(|error| db_error("could not commit the model refresh", error))?;
    Ok(())
}

/// Set the model a feature uses, or clear it.
pub fn set_feature(
    connection: &Connection,
    id: &str,
    label: &str,
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> Result<()> {
    // `enabled` follows from having a model: a feature with a model is on, and clearing the model turns it
    // off. Two separate switches would let the database hold a feature that is "enabled" with no model,
    // which is the state §11.3 exists to avoid.
    let enabled = provider_id.is_some() && model_id.is_some();
    connection
        .execute(
            "INSERT INTO ai_features (id, label, provider_id, model_id, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
               label = excluded.label,
               provider_id = excluded.provider_id,
               model_id = excluded.model_id,
               enabled = excluded.enabled",
            rusqlite::params![
                id,
                label,
                provider_id,
                model_id,
                if enabled { 1 } else { 0 }
            ],
        )
        .map_err(|error| db_error("could not write the AI feature mapping", error))?;
    Ok(())
}

/// Set the single global fallback (§11.4).
pub fn set_fallback(
    connection: &Connection,
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO ai_fallback (id, provider_id, model_id) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET
               provider_id = excluded.provider_id,
               model_id = excluded.model_id",
            rusqlite::params![provider_id, model_id],
        )
        .map_err(|error| db_error("could not write the AI fallback", error))?;
    Ok(())
}

/// One entry for the request audit.
///
/// A struct rather than a long positional argument list, so a caller cannot transpose the provider and the
/// model — which are both `Option<&str>` and would compile silently.
#[derive(Debug, Clone)]
pub struct AiRequestRecord<'a> {
    pub id: &'a str,
    pub feature_id: &'a str,
    pub provider_id: Option<&'a str>,
    pub model: Option<&'a str>,
    pub status: &'a str,
    pub used_fallback: bool,
    pub created_at: &'a str,
}

/// Record a request in the audit table.
///
/// `handoff.md` §11.6 asks for the record. Only the OUTCOME is stored, never the prompt or the response: a
/// prompt can contain content and a response can contain anything, and this table exists to answer "which
/// model served this feature and did it fall back".
pub fn record_request(connection: &Connection, record: &AiRequestRecord<'_>) -> Result<()> {
    connection
        .execute(
            "INSERT INTO ai_requests
               (id, feature_id, provider_id, model, status, used_fallback, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                record.id,
                record.feature_id,
                record.provider_id,
                record.model,
                record.status,
                if record.used_fallback { 1 } else { 0 },
                record.created_at,
            ],
        )
        .map_err(|error| db_error("could not record the AI request", error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> Connection {
        crate::open_in_memory().expect("an in-memory database")
    }

    fn provider(id: &str, enabled: bool) -> AiProviderRow {
        AiProviderRow {
            id: id.to_string(),
            name: format!("Provider {id}"),
            protocol: "openai".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            credential_ref: Some(format!("veloct:{id}:key")),
            enabled,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn an_empty_database_reads_as_unconfigured_rather_than_failing() {
        // The first thing a new project does is ask, and the answer is "nothing configured" rather than an
        // error about a missing row.
        let connection = connection();
        let settings = read_settings(&connection).unwrap();
        assert!(settings.providers.is_empty());
        assert!(settings.models.is_empty());
        assert!(settings.features.is_empty());
        assert!(!settings.fallback.is_configured());
        assert!(!settings.has_any_provider());
    }

    #[test]
    fn a_provider_round_trips_with_a_credential_handle_and_never_a_secret() {
        // handoff.md section 12.2: the row names the credential, it does not hold it.
        let connection = connection();
        upsert_provider(&connection, &provider("p1", true)).unwrap();

        let settings = read_settings(&connection).unwrap();
        assert_eq!(settings.providers.len(), 1);
        let stored = &settings.providers[0];
        assert_eq!(stored.id, "p1");
        assert_eq!(stored.credential_ref.as_deref(), Some("veloct:p1:key"));
        assert!(stored.is_usable());
        assert!(settings.has_any_provider());
    }

    #[test]
    fn the_schema_has_nowhere_to_put_a_secret() {
        // A signature that accepted a credential value would make it easy to write a key into the database,
        // where it would be backed up and shipped. The absence is the guarantee.
        let connection = connection();
        let mut statement = connection
            .prepare("SELECT name FROM pragma_table_info('ai_providers')")
            .unwrap();
        let columns: Vec<String> = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .flatten()
            .collect();
        assert!(columns.contains(&"credential_ref".to_string()));
        for forbidden in ["api_key", "key", "secret", "token", "credential", "password"] {
            assert!(
                !columns.iter().any(|column| column == forbidden),
                "ai_providers must not have a {forbidden} column: {columns:?}"
            );
        }
    }

    #[test]
    fn upserting_a_provider_updates_it_rather_than_duplicating_it() {
        let connection = connection();
        upsert_provider(&connection, &provider("p1", true)).unwrap();

        let mut changed = provider("p1", false);
        changed.name = "Renamed".to_string();
        changed.base_url = "https://other.example.com/v1".to_string();
        upsert_provider(&connection, &changed).unwrap();

        let settings = read_settings(&connection).unwrap();
        assert_eq!(settings.providers.len(), 1, "still one row");
        assert_eq!(settings.providers[0].name, "Renamed");
        assert_eq!(settings.providers[0].base_url, "https://other.example.com/v1");
        assert!(!settings.providers[0].is_usable(), "a disabled provider is not usable");
        assert!(!settings.has_any_provider());
    }

    #[test]
    fn deleting_a_provider_clears_the_features_that_pointed_at_it() {
        // A feature pointing at a provider that is gone would be a configuration that cannot work and
        // cannot be explained.
        let connection = connection();
        upsert_provider(&connection, &provider("p1", true)).unwrap();
        set_feature(&connection, "page_create", "Page creation", Some("p1"), Some("m1")).unwrap();
        set_fallback(&connection, Some("p1"), Some("m1")).unwrap();

        delete_provider(&connection, "p1").unwrap();

        let settings = read_settings(&connection).unwrap();
        assert!(settings.providers.is_empty());
        let feature = settings.feature("page_create").unwrap();
        assert!(!feature.is_configured(), "{feature:?}");
        assert!(feature.provider_id.is_none());
        assert!(!settings.fallback.is_configured());
    }

    #[test]
    fn the_model_list_is_replaced_so_a_withdrawn_model_cannot_stay_selectable() {
        let mut connection = connection();
        upsert_provider(&connection, &provider("p1", true)).unwrap();

        let first = vec![
            AiModelRow {
                id: "m1".to_string(),
                provider_id: "p1".to_string(),
                model_id: "gpt-4o".to_string(),
                display_name: Some("GPT-4o".to_string()),
                source: "discovered".to_string(),
                context_window: Some(128_000),
                last_seen_at: Some("2026-01-01T00:00:00Z".to_string()),
            },
            AiModelRow {
                id: "m2".to_string(),
                provider_id: "p1".to_string(),
                model_id: "gpt-4o-mini".to_string(),
                display_name: None,
                source: "discovered".to_string(),
                context_window: None,
                last_seen_at: None,
            },
        ];
        replace_models(&mut connection, "p1", &first).unwrap();
        assert_eq!(read_models(&connection).unwrap().len(), 2);

        // The provider no longer offers gpt-4o.
        let second = vec![first[1].clone()];
        replace_models(&mut connection, "p1", &second).unwrap();

        let models = read_models(&connection).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_id, "gpt-4o-mini");
        // A model with no display name falls back to its id.
        assert_eq!(models[0].label(), "gpt-4o-mini");
    }

    #[test]
    fn a_model_with_a_display_name_labels_itself_with_it() {
        let mut connection = connection();
        upsert_provider(&connection, &provider("p1", true)).unwrap();
        replace_models(
            &mut connection,
            "p1",
            &[AiModelRow {
                id: "m1".to_string(),
                provider_id: "p1".to_string(),
                model_id: "gpt-4o".to_string(),
                display_name: Some("GPT-4o".to_string()),
                source: "discovered".to_string(),
                context_window: None,
                last_seen_at: None,
            }],
        )
        .unwrap();

        let settings = read_settings(&connection).unwrap();
        assert_eq!(settings.models_of("p1").len(), 1);
        assert_eq!(settings.models_of("p1")[0].label(), "GPT-4o");
        assert!(settings.models_of("p2").is_empty());
    }

    #[test]
    fn enabling_a_feature_follows_from_having_a_model_rather_than_being_a_second_switch() {
        // Two switches would let the database hold a feature that is "enabled" with no model, which is the
        // state handoff.md section 11.3 exists to avoid.
        let connection = connection();
        // The provider must exist first: the schema has a real foreign key.
        upsert_provider(&connection, &provider("p1", true)).unwrap();
        set_feature(&connection, "page_create", "Page creation", Some("p1"), Some("m1")).unwrap();
        let feature = read_settings(&connection).unwrap().feature("page_create").cloned().unwrap();
        assert!(feature.enabled);
        assert!(feature.is_configured());

        // Clearing the model turns it off.
        set_feature(&connection, "page_create", "Page creation", None, None).unwrap();
        let feature = read_settings(&connection).unwrap().feature("page_create").cloned().unwrap();
        assert!(!feature.enabled);
        assert!(!feature.is_configured());
    }

    #[test]
    fn a_feature_with_only_a_provider_is_not_configured() {
        let connection = connection();
        upsert_provider(&connection, &provider("p1", true)).unwrap();
        set_feature(&connection, "page_create", "Page creation", Some("p1"), None).unwrap();
        let feature = read_settings(&connection).unwrap().feature("page_create").cloned().unwrap();
        assert!(!feature.is_configured());
    }

    #[test]
    fn there_is_exactly_one_fallback_row() {
        // handoff.md section 11.4 calls it the single global fallback, and the schema enforces `id = 1`.
        let connection = connection();
        upsert_provider(&connection, &provider("p1", true)).unwrap();
        upsert_provider(&connection, &provider("p2", true)).unwrap();
        set_fallback(&connection, Some("p1"), Some("m1")).unwrap();
        set_fallback(&connection, Some("p2"), Some("m2")).unwrap();

        let settings = read_settings(&connection).unwrap();
        assert_eq!(settings.fallback.provider_id.as_deref(), Some("p2"));
        assert_eq!(settings.fallback.model_id.as_deref(), Some("m2"));

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM ai_fallback", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn reading_the_fallback_when_no_row_exists_reports_unconfigured() {
        let connection = connection();
        assert!(!read_fallback(&connection).unwrap().is_configured());
    }

    #[test]
    fn the_settings_load_into_the_ai_service_shapes() {
        // The settings are read by the shell to build an AiService, so the mapping has to exist for a
        // configured feature and NOT for an unconfigured one (handoff.md section 11.3).
        let connection = connection();
        upsert_provider(&connection, &provider("p1", true)).unwrap();
        set_feature(&connection, "page_create", "Page creation", Some("p1"), Some("gpt-4o")).unwrap();
        set_fallback(&connection, Some("p1"), Some("gpt-4o-mini")).unwrap();

        let settings = read_settings(&connection).unwrap();
        let feature = settings.feature("page_create").unwrap();
        assert!(feature.is_configured());
        assert_eq!(feature.provider_id.as_deref(), Some("p1"));
        assert_eq!(feature.model_id.as_deref(), Some("gpt-4o"));
        assert!(settings.fallback.is_configured());
        assert_eq!(settings.fallback.model_id.as_deref(), Some("gpt-4o-mini"));
        assert!(settings.feature("a_feature_that_was_never_configured").is_none());
    }

    #[test]
    fn a_request_is_recorded_by_outcome_and_not_by_content() {
        // handoff.md section 11.6 asks for the record. A prompt can contain content and a response can
        // contain anything, so the audit answers "which model served this and did it fall back".
        let connection = connection();
        record_request(
            &connection,
            &AiRequestRecord {
                id: "r1",
                feature_id: "page_create",
                provider_id: Some("p1"),
                model: Some("gpt-4o"),
                status: "fallback_succeeded",
                used_fallback: true,
                created_at: "2026-01-01T00:00:00Z",
            },
        )
        .unwrap();

        let (status, used_fallback, feature): (String, i64, String) = connection
            .query_row(
                "SELECT status, used_fallback, feature_id FROM ai_requests WHERE id = 'r1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "fallback_succeeded");
        assert_eq!(used_fallback, 1);
        assert_eq!(feature, "page_create");

        // And there is no column for the prompt or the response.
        let mut statement = connection
            .prepare("SELECT name FROM pragma_table_info('ai_requests')")
            .unwrap();
        let columns: Vec<String> = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .flatten()
            .collect();
        for forbidden in ["prompt", "response", "body", "content"] {
            assert!(
                !columns.contains(&forbidden.to_string()),
                "ai_requests must not store {forbidden}: {columns:?}"
            );
        }
    }

    #[test]
    fn a_feature_cannot_point_at_a_provider_that_does_not_exist() {
        // The constraint is what makes "the settings are consistent" a property of the database rather than
        // of every caller remembering to check.
        let connection = connection();
        let error = set_feature(&connection, "page_create", "Page creation", Some("ghost"), Some("m1"))
            .unwrap_err();
        assert!(error.to_string().contains("FOREIGN KEY"), "{error}");
    }

    #[test]
    fn providers_are_listed_deterministically() {
        // A UI that reordered itself between reads would be unusable.
        let connection = connection();
        for id in ["p3", "p1", "p2"] {
            let mut row = provider(id, true);
            row.name = format!("Name {id}");
            upsert_provider(&connection, &row).unwrap();
        }
        let ids: Vec<String> = read_providers(&connection)
            .unwrap()
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(ids, vec!["p1", "p2", "p3"]);
    }
}
