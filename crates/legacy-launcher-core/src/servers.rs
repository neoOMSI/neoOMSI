use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct ServerEntry {
    pub name: String,
    pub address: String,
}

pub fn with_official(mut list: Vec<ServerEntry>) -> Vec<ServerEntry> {
    if !list
        .iter()
        .any(|s| network::official::is_alias(&s.address))
    {
        list.insert(
            0,
            ServerEntry {
                name: network::official::NAME.into(),
                address: network::official::ALIAS.into(),
            },
        );
    }
    list
}

fn path() -> std::path::PathBuf {
    crate::data_dir().join("servers.json")
}

pub fn load() -> Vec<ServerEntry> {
    with_official(
        std::fs::read(path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default(),
    )
}

/// The game's own launcher reads it too: never half written.
pub fn store(servers: &[ServerEntry]) -> std::io::Result<()> {
    let p = path();
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(servers).unwrap_or_default())?;
    std::fs::rename(&tmp, &p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_official_server_is_added_once() {
        let mine = ServerEntry {
            name: "Mine".into(),
            address: "http://10.0.0.2:27025".into(),
        };
        let list = with_official(vec![mine.clone()]);
        assert_eq!(list.len(), 2);
        assert!(network::official::is_alias(&list[0].address));
        assert_eq!(list[1], mine);
        assert_eq!(with_official(list.clone()), list);
    }
}
