//! The journey of a waiting passenger transferred to a client's bus. Names are UTF-8
//! encoded as hex so map-defined punctuation remains part of the identity.

#[derive(Debug, Clone, PartialEq)]
pub struct PassengerGrant {
    pub id: u32,
    pub transfer: u64,
    pub stop: i64,
    pub spot: u32,
    pub destination: Option<String>,
    pub alternative: Option<String>,
    pub alternative_m: f32,
    pub ride_km: f32,
    /// The semantic name of the stop's line record, never its local array index.
    pub line_destination: Option<String>,
    /// None: no line restriction; Some: the HOF terminus identities this rider accepts.
    pub allowed_termini: Option<Vec<String>>,
}

fn encode_name(name: &Option<String>) -> String {
    match name {
        None => "-".into(),
        Some(s) => s.bytes().map(|b| format!("{b:02x}")).collect(),
    }
}

fn decode_name(s: &str) -> Option<Option<String>> {
    if s == "-" {
        return Some(None);
    }
    if s.len() % 2 != 0 {
        return None;
    }
    let bytes = (0..s.len())
        .step_by(2)
        .map(|k| u8::from_str_radix(s.get(k..k + 2)?, 16).ok())
        .collect::<Option<Vec<_>>>()?;
    Some(Some(String::from_utf8(bytes).ok()?))
}

impl PassengerGrant {
    pub fn encode(&self) -> Option<String> {
        if !self.ride_km.is_finite()
            || self.ride_km <= 0.0
            || !self.alternative_m.is_finite()
            || self.alternative_m < 0.0
        {
            return None;
        }
        let allowed = match &self.allowed_termini {
            None => "-".into(),
            Some(names) => format!(
                "{};{}",
                names.len(),
                names
                    .iter()
                    .map(|n| encode_name(&Some(n.clone())))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        };
        let text = format!(
            "GRANT|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.id,
            self.transfer,
            self.stop,
            self.spot,
            self.ride_km,
            self.alternative_m,
            encode_name(&self.destination),
            encode_name(&self.alternative),
            encode_name(&self.line_destination),
            allowed
        );
        (text.len() <= crate::MAX_DATAGRAM).then_some(text)
    }

    pub fn decode(parts: &[&str]) -> Option<Self> {
        if parts.len() != 11 || parts[0] != "GRANT" {
            return None;
        }
        let grant = Self {
            id: parts[1].parse().ok()?,
            transfer: parts[2].parse().ok()?,
            stop: parts[3].parse().ok()?,
            spot: parts[4].parse().ok()?,
            ride_km: parts[5].parse().ok()?,
            alternative_m: parts[6].parse().ok()?,
            destination: decode_name(parts[7])?,
            alternative: decode_name(parts[8])?,
            line_destination: decode_name(parts[9])?,
            allowed_termini: match parts[10] {
                "-" => None,
                text => {
                    let (count, text) = text.split_once(';')?;
                    let count: usize = count.parse().ok()?;
                    let names = if count == 0 && text.is_empty() {
                        vec![]
                    } else {
                        text.split(',')
                            .map(|name| decode_name(name).flatten())
                            .collect::<Option<Vec<_>>>()?
                    };
                    if count != names.len() {
                        return None;
                    }
                    Some(names)
                }
            },
        };
        grant.encode()?;
        Some(grant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journey_names_and_distance_survive_the_handover() {
        let grant = PassengerGrant {
            id: 7,
            transfer: 42,
            stop: -42,
            spot: 2,
            ride_km: 7.25,
            destination: Some("Königsrath, Bf. Ausstieg|测试".into()),
            alternative: Some("Bf. Pause_#terminus".into()),
            alternative_m: 500.0,
            line_destination: Some("153: Königsrath".into()),
            allowed_termini: Some(vec!["KÖNIGSRATH".into(), "站台".into()]),
        };
        let wire = grant.encode().unwrap();
        assert_eq!(
            PassengerGrant::decode(&wire.split('|').collect::<Vec<_>>()),
            Some(grant)
        );
    }

    #[test]
    fn invalid_or_truncated_journeys_are_rejected() {
        for text in [
            "GRANT|7|42|1|0|0|0|-|-|-|-",
            "GRANT|7|42|1|0|NaN|0|-|-|-|-",
            "GRANT|7|42|1|0|1|-1|-|-|-|-",
            "GRANT|7|42|1|0|1|0|ff|-|-|-",
            "GRANT|7|1",
            "GRANT|7|42|1|0|1|0|-|-|-|2;41",
            "GRANT|7|42|1|0|1|0|-|-|-|1;ff",
        ] {
            assert!(
                PassengerGrant::decode(&text.split('|').collect::<Vec<_>>()).is_none(),
                "{text}"
            );
        }
    }

    #[test]
    fn oversized_journeys_are_rejected_in_both_directions() {
        let destination = "A".repeat(crate::MAX_DATAGRAM);
        let grant = PassengerGrant {
            id: 7,
            transfer: 42,
            stop: 1,
            spot: 0,
            ride_km: 1.0,
            destination: Some(destination.clone()),
            alternative: None,
            alternative_m: 0.0,
            line_destination: None,
            allowed_termini: None,
        };
        assert!(grant.encode().is_none());
        let text = format!(
            "GRANT|7|42|1|0|1|0|{}|-|-|-",
            "41".repeat(destination.len())
        );
        assert!(PassengerGrant::decode(&text.split('|').collect::<Vec<_>>()).is_none());
    }
}
