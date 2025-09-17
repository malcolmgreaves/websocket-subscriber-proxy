use std::{convert, fmt, hash, num};

use dashmap::{DashMap, Entry};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

// // // // // // // // // // // // // // // // // // // // // // // // // //
//                                                                         //
//                               UUID v4                                   //
//                                                                         //
// // // // // // // // // // // // // // // // // // // // // // // // // //
//
#[allow(non_upper_case_globals)]
const UUIDv4_REGEX_PATTERN: &str = r"[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";

lazy_static! {
    static ref UUIDV4_REGEX: Regex = Regex::new(UUIDv4_REGEX_PATTERN).unwrap();
}

/// True iff the string is a valid UUIDv4. False otherwise.
pub fn is_valid_uuid4(s: &str) -> bool {
    s.len() == 36 && UUIDV4_REGEX.is_match(s)
}

// // // // // // // // // // // // // // // // // // // // // // // // // //
//                                                                         //
//                          Id: String Newtype                             //
//                                                                         //
// // // // // // // // // // // // // // // // // // // // // // // // // //

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(String);

impl Id {
    /// Generates a new UUIDv4.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Borrow the underlying UUID string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    /// Display the underlying UUID only.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for Id {
    /// Generates a new UUIDv4.
    fn default() -> Self {
        Self::new()
    }
}

impl convert::TryFrom<String> for Id {
    type Error = crate::errors::Error;

    /// Convert an existing string into an `Id` iff it is a valid UUIDv4.
    ///
    /// If the string is not a valid UUIDv4, an `InvalidId` error is returned.
    /// Uses a regular expression to validate the UUID.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !is_valid_uuid4(&value) {
            Err(Self::Error::InvalidId(value))
        } else {
            Ok(Self(value))
        }
    }
}

impl convert::TryFrom<&str> for Id {
    type Error = crate::errors::Error;

    /// Converts an existing string value into an `Id` iff it is a valid UUIDv4.
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl From<Id> for String {
    /// Copies the underlying UUID string.
    fn from(value: Id) -> Self {
        value.0
    }
}

impl AsRef<str> for Id {
    /// Borrows the underlying UUID string.
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for Id {
    type Target = str;

    /// Borrows the underlying UUID string.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for Id {
    /// Serializes by only using the underlying v4 UUID string.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Id {
    /// Deserializes by treating the input string as a v4 UUID.
    ///
    /// Uses the `TryFrom` implementation for validation.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::try_from(s).map_err(serde::de::Error::custom)
    }
}

// // // // // // // // // // // // // // // // // // // // // // // // // //
//                                                                         //
//                               DashCounter                               //
//                                                                         //
// // // // // // // // // // // // // // // // // // // // // // // // // //

/// A threadsafe counter.
///
/// Uses a DashMap under the hood for thread safety. Maps keys to a non-zero count.
#[derive(Debug, Clone)]
pub struct DashCounter<K: hash::Hash + Eq> {
    count: DashMap<K, num::NonZeroU32>,
}

/// Attempts to decrement a non-zero u32 in-place. True means it was decremented.
///
/// If the value is 1, then decrementing it would violate its non-zero invariant.
/// In this situation, this function does nothing and returns false.
///
/// If the function returns true, it means that the value was more than 1.
/// When true is returned, the value will always be decremented by exactly 1.
pub fn decrement_in_place(nz: &mut num::NonZeroU32) -> bool {
    let v = nz.get();
    if v > 1 {
        // SAFETY: v > 1 ==> v - 1 != 0
        *nz = unsafe { num::NonZeroU32::new_unchecked(v - 1) };
        true
    } else {
        false
    }
}

/// Attempts to increment a non-zero u32 in-place. True means it was incremented.
///
/// If the value is u32::MAX, then incrementing it would overflow.
/// In this situation, this function does nothing and returns false.
///
/// If the function returns true, it means that the value was less than u32::MAX.
/// When true is returned, the value will always be incremented by exactly 1.
pub fn increment_in_place(nz: &mut num::NonZeroU32) -> bool {
    let v = nz.get();
    if v < u32::MAX {
        // SAFETY: v < u32::MAX ==> v + 1 <= u32::MAX
        *nz = unsafe { num::NonZeroU32::new_unchecked(v + 1) };
        true
    } else {
        false
    }
}

const ONE: num::NonZeroU32 = num::NonZeroU32::new(1).unwrap();

impl<K> DashCounter<K>
where
    K: hash::Hash + Eq + Clone,
{
    pub fn new() -> Self {
        DashCounter { count: DashMap::new() }
    }

    /// Increments the count of the key by 1.
    ///
    /// Maximum count value is u32::MAX.
    pub fn increment(&self, key: K) {
        match self.count.entry(key) {
            Entry::Occupied(mut entry) => {
                increment_in_place(entry.get_mut());
            }
            Entry::Vacant(entry) => {
                entry.insert(ONE.clone());
            }
        }
    }

    /// Decrements the count of the key by 1.
    ///
    /// Minimum count value is zero.
    pub fn decrement(&self, key: K) {
        match self.count.entry(key) {
            Entry::Occupied(mut entry) => {
                if decrement_in_place(entry.get_mut()) {
                    // it was decremented
                    ()
                } else {
                    // decrementing it would make it zero
                    // ==> remove it from the map as the count is now zero
                    entry.remove();
                }
            }
            Entry::Vacant(_) => (),
        }
    }

    /// True means the key has some non-zero count. False means the key has a count of zero.
    pub fn contains(&self, key: &K) -> bool {
        self.count.contains_key(key)
    }

    /// Returns the count of the key. A count of zero means the key is not present.
    #[allow(dead_code)]
    pub fn count_of(&self, key: &K) -> u32 {
        self.count.get(key).map(|k| k.get()).unwrap_or(0)
    }
}
