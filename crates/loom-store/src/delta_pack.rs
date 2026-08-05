use crate::{Result, corrupt, get_uvarint, put_uvarint};
use loom_core::{Algo, Digest};
use std::collections::BTreeSet;

const MAGIC: &[u8; 8] = b"LDPACK1\0";
const MAX_MEMBERS: usize = 512;
const ADDRESS_DOMAIN: &[u8] = b"loom.store.delta-pack-advisory.v1";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PackMember {
    pub(crate) family_id: u16,
    pub(crate) address: [u8; 32],
    pub(crate) digest: [u8; 32],
    pub(crate) slot: u32,
    pub(crate) payload_len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackAdvisory {
    pub(crate) page: u64,
    pub(crate) generation: u64,
    pub(crate) members: Vec<PackMember>,
    pub(crate) dead_slots: BTreeSet<u32>,
}

impl PackAdvisory {
    pub(crate) fn new(
        page: u64,
        generation: u64,
        mut members: Vec<PackMember>,
        dead_slots: BTreeSet<u32>,
    ) -> Result<Self> {
        members.sort();
        let advisory = Self {
            page,
            generation,
            members,
            dead_slots,
        };
        advisory.validate()?;
        Ok(advisory)
    }

    pub(crate) fn address(algo: Algo, page: u64) -> [u8; 32] {
        let mut key = Vec::with_capacity(ADDRESS_DOMAIN.len() + 8);
        key.extend_from_slice(ADDRESS_DOMAIN);
        key.extend_from_slice(&page.to_be_bytes());
        *Digest::hash(algo, &key).bytes()
    }

    pub(crate) fn has_debt(&self) -> bool {
        let live_bytes = self
            .members
            .iter()
            .filter(|member| !self.dead_slots.contains(&member.slot))
            .map(|member| u64::from(member.payload_len).saturating_add(4))
            .sum::<u64>();
        !self.dead_slots.is_empty() || live_bytes < 3_072
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut out = Vec::with_capacity(MAGIC.len() + 24 + self.members.len() * 72);
        out.extend_from_slice(MAGIC);
        put_uvarint(&mut out, self.page);
        put_uvarint(&mut out, self.generation);
        put_uvarint(&mut out, self.members.len() as u64);
        for member in &self.members {
            out.extend_from_slice(&member.family_id.to_be_bytes());
            out.extend_from_slice(&member.address);
            out.extend_from_slice(&member.digest);
            put_uvarint(&mut out, u64::from(member.slot));
            put_uvarint(&mut out, u64::from(member.payload_len));
        }
        put_uvarint(&mut out, self.dead_slots.len() as u64);
        for slot in &self.dead_slots {
            put_uvarint(&mut out, u64::from(*slot));
        }
        Ok(out)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if !bytes.starts_with(MAGIC) {
            return Err(corrupt("delta-pack advisory schema mismatch"));
        }
        let mut pos = MAGIC.len();
        let page = get_uvarint(bytes, &mut pos)
            .ok_or_else(|| corrupt("delta-pack advisory page truncated"))?;
        let generation = get_uvarint(bytes, &mut pos)
            .ok_or_else(|| corrupt("delta-pack advisory generation truncated"))?;
        let member_count = bounded_count(bytes, &mut pos, "member")?;
        let mut members = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            let family_end = pos
                .checked_add(2)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| corrupt("delta-pack advisory family truncated"))?;
            let family_id = u16::from_be_bytes(bytes[pos..family_end].try_into().unwrap());
            pos = family_end;
            let address_end = pos
                .checked_add(32)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| corrupt("delta-pack advisory address truncated"))?;
            let address = bytes[pos..address_end].try_into().unwrap();
            pos = address_end;
            let digest_end = pos
                .checked_add(32)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| corrupt("delta-pack advisory digest truncated"))?;
            let digest = bytes[pos..digest_end].try_into().unwrap();
            pos = digest_end;
            let slot = u32::try_from(
                get_uvarint(bytes, &mut pos)
                    .ok_or_else(|| corrupt("delta-pack advisory slot truncated"))?,
            )
            .map_err(|_| corrupt("delta-pack advisory slot out of range"))?;
            let payload_len = u32::try_from(
                get_uvarint(bytes, &mut pos)
                    .ok_or_else(|| corrupt("delta-pack advisory payload length truncated"))?,
            )
            .map_err(|_| corrupt("delta-pack advisory payload length out of range"))?;
            members.push(PackMember {
                family_id,
                address,
                digest,
                slot,
                payload_len,
            });
        }
        let dead_count = bounded_count(bytes, &mut pos, "dead-slot")?;
        let mut dead_slots = BTreeSet::new();
        for _ in 0..dead_count {
            let slot = u32::try_from(
                get_uvarint(bytes, &mut pos)
                    .ok_or_else(|| corrupt("delta-pack advisory dead slot truncated"))?,
            )
            .map_err(|_| corrupt("delta-pack advisory dead slot out of range"))?;
            if !dead_slots.insert(slot) {
                return Err(corrupt("delta-pack advisory duplicate dead slot"));
            }
        }
        if pos != bytes.len() {
            return Err(corrupt("delta-pack advisory trailing bytes"));
        }
        let advisory = Self::new(page, generation, members, dead_slots)?;
        if advisory.encode()? != bytes {
            return Err(corrupt("delta-pack advisory is not canonical"));
        }
        Ok(advisory)
    }

    fn validate(&self) -> Result<()> {
        if self.members.is_empty() || self.members.len() > MAX_MEMBERS {
            return Err(corrupt("delta-pack advisory member count invalid"));
        }
        let mut identities = BTreeSet::new();
        let mut slots = BTreeSet::new();
        let mut previous = None;
        for member in &self.members {
            if previous.is_some_and(|value| value >= member) {
                return Err(corrupt("delta-pack advisory members are not canonical"));
            }
            if !identities.insert((member.family_id, member.address)) || !slots.insert(member.slot)
            {
                return Err(corrupt("delta-pack advisory member identity is duplicated"));
            }
            previous = Some(member);
        }
        if self.dead_slots.iter().any(|slot| !slots.contains(slot)) {
            return Err(corrupt("delta-pack advisory dead slot has no member"));
        }
        Ok(())
    }
}

fn bounded_count(bytes: &[u8], pos: &mut usize, name: &str) -> Result<usize> {
    let count = get_uvarint(bytes, pos)
        .ok_or_else(|| corrupt(&format!("delta-pack advisory {name} count truncated")))?;
    usize::try_from(count)
        .ok()
        .filter(|count| *count <= MAX_MEMBERS)
        .ok_or_else(|| corrupt(&format!("delta-pack advisory {name} count invalid")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(family_id: u16, address: u8, digest: u8, slot: u32) -> PackMember {
        PackMember {
            family_id,
            address: [address; 32],
            digest: [digest; 32],
            slot,
            payload_len: 17,
        }
    }

    #[test]
    fn physical_advisory_is_canonical_and_round_trips() {
        let advisory = PackAdvisory::new(
            41,
            9,
            vec![member(3, 2, 8, 1), member(2, 1, 7, 0)],
            BTreeSet::from([1]),
        )
        .unwrap();
        let encoded = advisory.encode().unwrap();
        assert_eq!(PackAdvisory::decode(&encoded).unwrap(), advisory);
        assert_eq!(advisory.members[0].family_id, 2);
        assert!(advisory.has_debt());
        assert_eq!(
            PackAdvisory::address(Algo::Blake3, 41),
            PackAdvisory::address(Algo::Blake3, 41)
        );
    }

    #[test]
    fn physical_advisory_rejects_unknown_and_duplicate_dead_slots() {
        assert!(PackAdvisory::new(41, 9, vec![member(2, 1, 7, 0)], BTreeSet::from([1])).is_err());
        let mut encoded = PackAdvisory::new(41, 9, vec![member(2, 1, 7, 0)], BTreeSet::from([0]))
            .unwrap()
            .encode()
            .unwrap();
        *encoded.last_mut().unwrap() = 1;
        assert!(PackAdvisory::decode(&encoded).is_err());
    }
}
