//! Import and export of custom resolvers and favourites.
//!
//! The file is JSON so it can be edited by hand and copied between machines. Import validates
//! every entry and merges instead of replacing, so a bad file cannot wipe the current list.

use serde::{Deserialize, Serialize};

use crate::config::{CustomResolver, Settings};
use crate::servers::Protocol;

const FORMAT: &str = "dnsrace-resolvers";

#[derive(Serialize, Deserialize)]
struct ExportFile {
    format: String,
    version: u32,
    resolvers: Vec<CustomResolver>,
    favourites: Vec<String>,
}

/// What one import changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub added_resolvers: usize,
    pub added_favourites: usize,
    /// Entries that were already present or failed validation.
    pub skipped: usize,
}

pub fn export(settings: &Settings) -> Result<String, String> {
    let file = ExportFile {
        format: FORMAT.to_owned(),
        version: 1,
        resolvers: settings.custom.clone(),
        favourites: settings.favorites.iter().cloned().collect(),
    };
    serde_json::to_string_pretty(&file).map_err(|error| error.to_string())
}

pub fn import(text: &str, settings: &mut Settings) -> Result<ImportSummary, String> {
    let file: ExportFile =
        serde_json::from_str(text).map_err(|error| format!("invalid resolver file: {error}"))?;
    if file.format != FORMAT {
        return Err("not a dnsrace resolver export".to_owned());
    }
    let mut summary = ImportSummary::default();
    for resolver in file.resolvers {
        if !valid(&resolver) || settings.custom.iter().any(|old| same(old, &resolver)) {
            summary.skipped += 1;
            continue;
        }
        settings.custom.push(resolver);
        summary.added_resolvers += 1;
    }
    for id in file.favourites {
        if id.trim().is_empty() || !settings.favorites.insert(id) {
            summary.skipped += 1;
        } else {
            summary.added_favourites += 1;
        }
    }
    Ok(summary)
}

fn same(left: &CustomResolver, right: &CustomResolver) -> bool {
    left.name == right.name
        && left.protocol == right.protocol
        && left.endpoint == right.endpoint
        && left.address == right.address
}

fn valid(resolver: &CustomResolver) -> bool {
    let name = resolver.name.trim();
    let endpoint = resolver.endpoint.trim();
    if name.is_empty() || endpoint.is_empty() || endpoint.chars().any(char::is_whitespace) {
        return false;
    }
    match resolver.protocol {
        Protocol::Doh => endpoint.starts_with("https://") || endpoint.starts_with("http://"),
        Protocol::Dot => !endpoint.contains("://"),
        Protocol::Udp | Protocol::Tcp => endpoint.parse::<std::net::IpAddr>().is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    fn resolver(name: &str, endpoint: &str) -> CustomResolver {
        CustomResolver {
            name: name.into(),
            protocol: Protocol::Doh,
            endpoint: endpoint.into(),
            address: None,
        }
    }

    #[test]
    fn export_import_round_trip_merges_without_duplicates() {
        let mut source = Settings {
            custom: vec![resolver("Mine", "https://dns.example/dns-query")],
            ..Settings::default()
        };
        source.favorites.insert("cloudflare".into());
        let text = export(&source).unwrap();

        let mut target = Settings::default();
        let summary = import(&text, &mut target).unwrap();
        assert_eq!(summary.added_resolvers, 1);
        assert_eq!(summary.added_favourites, 1);
        assert_eq!(target.custom.len(), 1);

        let again = import(&text, &mut target).unwrap();
        assert_eq!(again.added_resolvers, 0);
        assert_eq!(again.skipped, 2);
        assert_eq!(target.custom.len(), 1);
    }

    #[test]
    fn invalid_entries_are_rejected() {
        let text = format!(
            r#"{{"format":"{FORMAT}","version":1,"resolvers":[
                {{"name":"","protocol":"Doh","endpoint":"https://x"}},
                {{"name":"bad","protocol":"Udp","endpoint":"not-an-ip"}},
                {{"name":"ok","protocol":"Doh","endpoint":"https://dns.example/q"}}
            ],"favourites":[]}}"#
        );
        let mut settings = Settings::default();
        let summary = import(&text, &mut settings).unwrap();
        assert_eq!(summary.added_resolvers, 1);
        assert_eq!(summary.skipped, 2);
    }

    #[test]
    fn a_foreign_file_is_refused() {
        let mut settings = Settings::default();
        assert!(import("{\"format\":\"other\"}", &mut settings).is_err());
    }
}
