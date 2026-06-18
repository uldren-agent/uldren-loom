use std::cmp::Ordering;

use loom_types::{Code, LoomError, Result};

const TOKEN_WIDTH: usize = 32;
const MIN_VALUE: u128 = 0;
const MAX_VALUE: u128 = u128::MAX;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrderToken(String);

impl OrderToken {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        parse_token_value(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn value(&self) -> u128 {
        parse_token_value(&self.0).expect("OrderToken is validated at construction")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedEntity {
    pub entity_id: String,
    pub token: OrderToken,
}

impl OrderedEntity {
    pub fn new(entity_id: impl Into<String>, token: OrderToken) -> Result<Self> {
        let entity_id = entity_id.into();
        validate_entity_id(&entity_id)?;
        Ok(Self { entity_id, token })
    }
}

pub fn first_token() -> OrderToken {
    token_from_value(MAX_VALUE / 2)
}

pub fn insert_between(
    after: Option<&OrderToken>,
    before: Option<&OrderToken>,
) -> Result<OrderToken> {
    let lower = after.map(OrderToken::value).unwrap_or(MIN_VALUE);
    let upper = before.map(OrderToken::value).unwrap_or(MAX_VALUE);
    if lower >= upper {
        return Err(LoomError::invalid("order token bounds are reversed"));
    }
    if upper - lower <= 1 {
        return Err(LoomError::new(
            Code::Conflict,
            "order token gap exhausted; compact order first",
        ));
    }
    Ok(token_from_value(lower + ((upper - lower) / 2)))
}

pub fn compare_position(left: &OrderedEntity, right: &OrderedEntity) -> Ordering {
    left.token
        .cmp(&right.token)
        .then_with(|| left.entity_id.cmp(&right.entity_id))
}

pub fn compact<I>(entities: I) -> Result<Vec<OrderedEntity>>
where
    I: IntoIterator<Item = OrderedEntity>,
{
    let mut entities = entities.into_iter().collect::<Vec<_>>();
    entities.sort_by(compare_position);
    let len = entities.len();
    if len == 0 {
        return Ok(Vec::new());
    }
    let step = MAX_VALUE / ((len as u128) + 1);
    if step == 0 {
        return Err(LoomError::unsupported("too many entities to compact"));
    }
    Ok(entities
        .into_iter()
        .enumerate()
        .map(|(idx, entity)| OrderedEntity {
            entity_id: entity.entity_id,
            token: token_from_value(step * ((idx as u128) + 1)),
        })
        .collect())
}

fn token_from_value(value: u128) -> OrderToken {
    OrderToken(format!("{value:0TOKEN_WIDTH$X}"))
}

fn parse_token_value(value: &str) -> Result<u128> {
    if value.len() != TOKEN_WIDTH {
        return Err(LoomError::invalid("order token must be 32 hex characters"));
    }
    if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(LoomError::invalid("order token must be hexadecimal"));
    }
    u128::from_str_radix(value, 16)
        .map_err(|_| LoomError::invalid("order token is outside u128 range"))
}

fn validate_entity_id(entity_id: &str) -> Result<()> {
    if entity_id.is_empty() {
        return Err(LoomError::invalid("entity_id must not be empty"));
    }
    if entity_id.len() > 512 {
        return Err(LoomError::invalid("entity_id is too long"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(entity_id: &str, token: OrderToken) -> OrderedEntity {
        OrderedEntity::new(entity_id, token).unwrap()
    }

    #[test]
    fn insert_between_preserves_lexicographic_order() {
        let first = first_token();
        let before = insert_between(None, Some(&first)).unwrap();
        let after = insert_between(Some(&first), None).unwrap();
        let middle = insert_between(Some(&before), Some(&after)).unwrap();
        assert!(before < first);
        assert!(first < after);
        assert!(before < middle);
        assert!(middle < after);
        assert_eq!(before.as_str().len(), TOKEN_WIDTH);
        assert_eq!(after.as_str().len(), TOKEN_WIDTH);
    }

    #[test]
    fn compare_position_uses_entity_id_as_deterministic_tie_breaker() {
        let token = first_token();
        let mut entities = [entity("b", token.clone()), entity("a", token)];
        entities.sort_by(compare_position);
        assert_eq!(entities[0].entity_id, "a");
        assert_eq!(entities[1].entity_id, "b");
    }

    #[test]
    fn compact_preserves_order_and_respacing() {
        let shared = first_token();
        let high = insert_between(Some(&shared), None).unwrap();
        let compacted = compact(vec![
            entity("c", high),
            entity("b", shared.clone()),
            entity("a", shared),
        ])
        .unwrap();
        assert_eq!(
            compacted
                .iter()
                .map(|entity| entity.entity_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert!(compacted[0].token < compacted[1].token);
        assert!(compacted[1].token < compacted[2].token);
    }

    #[test]
    fn exhausted_gap_requires_compaction() {
        let lower = OrderToken::new("00000000000000000000000000000001").unwrap();
        let upper = OrderToken::new("00000000000000000000000000000002").unwrap();
        let err = insert_between(Some(&lower), Some(&upper)).unwrap_err();
        assert_eq!(err.code, loom_types::Code::Conflict);
    }

    #[test]
    fn invalid_tokens_are_rejected() {
        assert!(OrderToken::new("1").is_err());
        assert!(OrderToken::new("0000000000000000000000000000000G").is_err());
    }
}
