extern crate util;
extern crate crypto;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate parking_lot;
extern crate rand;
extern crate bigint;
#[macro_use]
extern crate log;
extern crate bincode;

use parking_lot::{Mutex, RwLock};
use bigint::hash::{H256, H520, H512};
use std::collections::{BTreeMap, HashMap, HashSet};
use bincode::{serialize, deserialize, Infinite};
use rand::{thread_rng, Rng};
use util::Hashable;
use util::config::SleepyConfig;
use crypto::{KeyPair, sign as crypto_sign, verify_public as crypto_vefify};
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::sync::Arc;
use std::time::Duration;
use std::fmt;

#[derive(Debug, PartialEq)]
pub enum Error {
    FutureHeight,
    FutureTime,
    MissParent,
    Duplicate,
    Malformated,
}

#[derive(Hash, Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct Transcation {}

#[derive(Hash, Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct Proof {
    pub timestamp: u64,
    pub key: H512,
    pub signature: H520,
}

#[derive(Hash, Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct Block {
    pub height: u64,
    pub proof: Proof,
    pub transactions: Vec<Transcation>,
    pub pre_hash: H256,
}

#[derive(Hash, Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct SignedBlock {
    pub block: Block,
    pub singer: H512,
    pub signature: H520,
}

impl Block {
    pub fn new(height: u64,
               timestamp: u64,
               pre_hash: H256,
               proof_key: H512,
               signature: H520)
               -> Block {
        let proof = Proof {
            timestamp: timestamp,
            key: proof_key,
            signature: signature,
        };
        Block {
            height: height,
            proof: proof,
            transactions: Vec::new(),
            pre_hash: pre_hash,
        }
    }

    pub fn sign(self, keypair: &KeyPair) -> SignedBlock {
        let encoded: Vec<u8> = serialize(&self, Infinite).unwrap();
        let sign_hash = encoded.sha3();
        let signature: H520 = crypto_sign(keypair.privkey(), &sign_hash).unwrap().into();

        SignedBlock {
            block: self,
            singer: *keypair.pubkey(),
            signature: signature,
        }
    }

    pub fn is_first(&self) -> Result<bool, Error> {
        if self.height == 1 {
            if self.pre_hash != H256::zero() {
                return Err(Error::Malformated);
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl SignedBlock {
    pub fn verify(&self) -> bool {
        let encoded: Vec<u8> = serialize(&self.block, Infinite).unwrap();
        let sign_hash = encoded.sha3();
        match crypto_vefify(&self.singer, &self.signature.into(), &sign_hash) {
            Ok(ret) => ret,
            _ => false,
        }
    }
}

impl Proof {
    pub fn verify(&self) -> bool {
        match crypto_vefify(&self.key,
                            &self.signature.into(),
                            &H256::from(self.timestamp)) {
            Ok(ret) => ret,
            _ => false,
        }
    }
}

impl ::std::ops::Deref for SignedBlock {
    type Target = Block;

    #[inline]
    fn deref(&self) -> &Block {
        &self.block
    }
}

#[derive(Debug)]
pub struct Chain {
    inner: ChainImpl,
    sender: Mutex<Sender<(u64, H256)>>,
    config: Arc<RwLock<SleepyConfig>>,
}

type ChainIndex = BTreeMap<u64, H256>;

#[derive(Debug)]
struct ChainImpl {
    blocks: RwLock<HashMap<H256, SignedBlock>>,
    timestamp_future: RwLock<BTreeMap<u64, HashSet<SignedBlock>>>,
    height_future: RwLock<BTreeMap<u64, HashSet<SignedBlock>>>,
    parent_future: RwLock<BTreeMap<H256, HashSet<SignedBlock>>>,
    forks: RwLock<BTreeMap<u64, Vec<H256>>>,
    main: RwLock<ChainIndex>,
    current_height: RwLock<u64>,
    current_hash: RwLock<H256>,
}

//TODO maintenance longest chain
//fetch miss parent
impl Chain {
    pub fn init(config: Arc<RwLock<SleepyConfig>>) -> Arc<Self> {
        let (sender, receiver) = channel();
        let chain = Arc::new(Chain {
                                 inner: ChainImpl {
                                     blocks: RwLock::new(HashMap::new()),
                                     timestamp_future: RwLock::new(BTreeMap::new()),
                                     height_future: RwLock::new(BTreeMap::new()),
                                     parent_future: RwLock::new(BTreeMap::new()),
                                     forks: RwLock::new(BTreeMap::new()),
                                     main: RwLock::new(BTreeMap::new()),
                                     current_height: RwLock::new(0),
                                     current_hash: RwLock::new(H256::zero()),
                                 },
                                 sender: Mutex::new(sender),
                                 config: config.clone(),
                             });
        let mario = chain.clone();
        thread::spawn(move || loop {
                          info!("mario maintenance!");
                          let (height, hash) = receiver.recv().unwrap();
                          mario.maintenance(height, hash);
                      });

        let future = chain.clone();
        thread::spawn(move || {
            info!("hanle future!");
            let dur = 1000 / config.read().hz;
            let dur = Duration::from_millis(dur);
            loop {
                thread::sleep(dur);
                future.handle_future();
            }
        });
        chain
    }

    pub fn insert(&self, block: &SignedBlock) -> Result<(), Error> {
        let encoded: Vec<u8> = serialize(block, Infinite).unwrap();
        let hash = encoded.sha3();
        let bh = block.height;
        {
            let mut blocks = self.inner.blocks.write();
            let mut current_height = self.inner.current_height.write();
            let mut current_hash = self.inner.current_hash.write();
            let mut forks = self.inner.forks.write();
            let mut main = self.inner.main.write();

            if blocks.contains_key(&hash) {
                return Err(Error::Duplicate);
            }

            if bh > *current_height + 1 {
                let mut height_future = self.inner.height_future.write();
                let future = height_future.entry(bh).or_insert_with(HashSet::new);
                future.insert(block.clone());
                return Err(Error::FutureHeight);
            }

            if block.proof.timestamp > self.config.read().timestamp_now() {
                let mut timestamp_future = self.inner.timestamp_future.write();
                let future = timestamp_future
                    .entry(block.proof.timestamp)
                    .or_insert_with(HashSet::new);
                future.insert(block.clone());
                return Err(Error::FutureTime);
            }
            let first_block = block.is_first()?;
            if !first_block && !blocks.contains_key(&block.pre_hash) {
                let mut parent_future = self.inner.parent_future.write();
                let future = parent_future
                    .entry(block.pre_hash)
                    .or_insert_with(HashSet::new);
                future.insert(block.clone());
                return Err(Error::MissParent);
            }
            if !first_block {
                let parent = blocks.get(&block.pre_hash).cloned().unwrap();
                if !(block.proof.timestamp > parent.proof.timestamp) {
                    return Err(Error::Malformated);
                }
            }

            if bh == *current_height + 1 {
                *current_height = bh;
                *current_hash = hash;
                main.insert(bh, hash);
                info!("insert a block {:?} {:?} {:?}",
                      bh,
                      hash,
                      block.proof.timestamp);
                let forks = forks.entry(bh).or_insert_with(Vec::new);
                forks.push(hash);
                // tmp impl:  rand pick a fork
                if forks.len() > 1 {
                    info!("we meet fork!");
                    let mut rng = thread_rng();
                    let n: usize = rng.gen_range(0, forks.len());
                    let pick = forks[n];
                    *current_hash = pick;
                    main.insert(bh, hash);

                    self.sender.lock().send((bh - 1, block.pre_hash));
                }
                // log
                info!("Chain {{");
                for (key, value) in main.iter().rev().take(10) {
                    info!("   {} => {}", key, value);
                }
                info!("}}");

            }
            blocks.insert(hash, block.clone());
        }

        let pendings = {
            let mut parent_future = self.inner.parent_future.write();
            if parent_future.contains_key(&hash) {
                parent_future.remove(&hash)
            } else {
                None
            }
        };

        pendings.map(|blks| for blk in blks {
                         let _ = self.insert(&blk);
                     });
        Ok(())
    }

    pub fn get_status(&self) -> (u64, H256) {
        let current_height = self.inner.current_height.read();
        let current_hash = self.inner.current_hash.read();
        (*current_height, *current_hash)
    }

    pub fn get_block_by_hash(&self, hash: &H256) -> Option<SignedBlock> {
        self.inner.blocks.read().get(hash).cloned()
    }

    pub fn maintenance(&self, height: u64, hash: H256) {
        let mut start_bh = height;
        let mut pre_hash = hash;
        let mut main = self.inner.main.write();

        if main.get(&start_bh) != Some(&hash) {
            main.insert(start_bh, hash);
            let blocks = self.inner.blocks.read();
            loop {
                let block = blocks.get(&pre_hash).cloned().unwrap();
                pre_hash = block.pre_hash;
                start_bh -= 1;
                info!("maintenance loop {} {}", start_bh, &pre_hash);
                if main.get(&start_bh) == Some(&pre_hash) {
                    break;
                }
                main.insert(start_bh, pre_hash);
            }
        }
    }

    pub fn handle_future(&self) {
        self.handle_timestamp_future();
        self.handle_height_future();
    }

    fn handle_timestamp_future(&self) {
        let pendings = {
            let mut timestamp_future = self.inner.timestamp_future.write();
            let now = self.config.read().timestamp_now();
            let new_future = timestamp_future.split_off(&now);
            let pendings = timestamp_future.clone();
            *timestamp_future = new_future;
            pendings
        };

        let blks: Vec<SignedBlock> = pendings
            .into_iter()
            .flat_map(|(_, s)| s.into_iter())
            .collect();

        for blk in blks {
            let _ = self.insert(&blk);
        }
    }

    fn handle_height_future(&self) {
        let pendings = {
            let mut height_future = self.inner.height_future.write();
            let height = self.inner.current_height.read();
            let new_future = height_future.split_off(&height);
            let pendings = height_future.clone();
            *height_future = new_future;
            pendings
        };

        let blks: Vec<SignedBlock> = pendings
            .into_iter()
            .flat_map(|(_, s)| s.into_iter())
            .collect();

        for blk in blks {
            let _ = self.insert(&blk);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_block_sig() {
        let privkey = H256::from("40f2d8f8e1594579824fd04edfc7ff1ddffd6be153b23f4318e1acff037d3ea9",);
        let keypair = KeyPair::from_privkey(privkey).unwrap();
        let message = H256::default();
        let timestamp = 12345;
        let sig = crypto_sign(keypair.privkey().into(), &H256::from(timestamp)).unwrap();
        let blk = Block::new(1, timestamp, message, *keypair.pubkey(), sig.into());
        assert_eq!(blk.proof.verify(), true);
        let signed_blk = blk.sign(&keypair);
        assert_eq!(signed_blk.verify(), true);
    }
}
