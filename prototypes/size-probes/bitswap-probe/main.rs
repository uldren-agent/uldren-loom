fn main() {
    #[cfg(feature = "baseline")]
    {
        println!("baseline: empty Bitswap probe");
    }

    #[cfg(feature = "bitswap")]
    {
        let behaviour = co_libp2p_bitswap::Bitswap::new(
            co_libp2p_bitswap::BitswapConfig::default(),
            ProbeStore,
            Box::new(|future| drop(future)),
        );
        println!("bitswap: type={}", std::any::type_name_of_val(&behaviour));
    }
}

#[cfg(feature = "bitswap")]
struct ProbeStore;

#[cfg(feature = "bitswap")]
#[async_trait::async_trait]
impl co_libp2p_bitswap::BitswapStore for ProbeStore {
    async fn contains(
        &mut self,
        _: &cid::Cid,
        _: &libp2p::PeerId,
        _: &[co_libp2p_bitswap::Token],
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    async fn get(
        &mut self,
        _: &cid::Cid,
        _: &libp2p::PeerId,
        _: &[co_libp2p_bitswap::Token],
    ) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn insert(
        &mut self,
        _: &co_libp2p_bitswap::Block,
        _: &libp2p::PeerId,
        _: &[co_libp2p_bitswap::Token],
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn missing_blocks(
        &mut self,
        _: &cid::Cid,
        _: &[co_libp2p_bitswap::Token],
    ) -> anyhow::Result<Vec<cid::Cid>> {
        Ok(Vec::new())
    }
}
