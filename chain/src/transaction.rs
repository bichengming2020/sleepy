use util::{H256, H512, H520, Hashable, HeapSizeOf};
use std::ops::{Deref, DerefMut};
use crypto::{recover, Signature};
use error::Error;

#[derive(Hash, Clone, Serialize, Deserialize, PartialEq, Eq, Debug, RlpEncodable, RlpDecodable)]
pub struct Transaction {
    /// Transaction data.
    pub data: Vec<u8>,
    pub hash: H256,
}

impl HeapSizeOf for Transaction {
    fn heap_size_of_children(&self) -> usize {
        self.data.heap_size_of_children()
    }
}

impl Transaction {
    pub fn new() -> Self {
        Transaction {
            hash: H256::default(),
            data: Vec::new()
        }
    }

    pub fn cal_hash(&self) -> H256 {
        self.data.sha3()
    }

    ///the hash of the transaction
    pub fn hash(&self) -> H256 {
        self.hash
    }

    ///set data
    pub fn set_data(&mut self, data: Vec<u8>) {
        self.data = data;
    }
}

#[derive(Hash, Clone, Serialize, Deserialize, PartialEq, Eq, Debug, RlpEncodable, RlpDecodable)]
pub struct SignedTransaction {
    pub transaction: Transaction,
    pub signature: H520,
}

impl HeapSizeOf for SignedTransaction {
    fn heap_size_of_children(&self) -> usize {
        self.transaction.heap_size_of_children()
    }
}

impl Deref for SignedTransaction {
    type Target = Transaction;

    fn deref(&self) -> &Transaction {
        &self.transaction
    }
}

impl DerefMut for SignedTransaction {
    fn deref_mut(&mut self) -> &mut Transaction {
        &mut self.transaction
    }
}

impl SignedTransaction {
    pub fn new() -> Self {
        SignedTransaction {
            transaction: Transaction::new(),
            signature: H520::default(),
        }
    }
    /// Recovers the public key of the sender.
	pub fn recover_public(&self) -> Result<H512, Error> {
        let sig: Signature = self.signature.into();
        recover(&sig, &self.hash()).map_err(|_| Error::InvalidSignature)
        
	}
}