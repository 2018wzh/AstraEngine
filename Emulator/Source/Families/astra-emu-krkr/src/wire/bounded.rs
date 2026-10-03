use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Bounds are applied while decoding, before allocating an attacker-supplied
/// element count. Encoded packet limits apply independently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Items<T, const N: usize>(Vec<T>);

impl<T, const N: usize> Items<T, N> {
    pub fn new(items: Vec<T>) -> Result<Self, &'static str> {
        if items.len() > N {
            return Err("item bound exceeded");
        }
        Ok(Self(items))
    }

    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<T> {
        self.0
    }
}

impl<T: Serialize, const N: usize> Serialize for Items<T, N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>, const N: usize> Deserialize<'de> for Items<T, N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<T, const N: usize>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>, const N: usize> de::Visitor<'de> for Visitor<T, N> {
            type Value = Items<T, N>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "at most {N} items")
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                if seq.size_hint().is_some_and(|size| size > N) {
                    return Err(de::Error::custom("item bound exceeded"));
                }
                let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(N));
                while let Some(item) = seq.next_element()? {
                    if items.len() == N {
                        return Err(de::Error::custom("item bound exceeded"));
                    }
                    items.push(item);
                }
                Ok(Items(items))
            }
        }
        deserializer.deserialize_seq(Visitor::<T, N>(std::marker::PhantomData))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text<const N: usize>(String);

impl<const N: usize> Text<N> {
    pub fn new(text: String) -> Result<Self, &'static str> {
        if text.len() > N {
            return Err("text bound exceeded");
        }
        Ok(Self(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<const N: usize> Serialize for Text<N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de, const N: usize> Deserialize<'de> for Text<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<const N: usize>;
        impl<const N: usize> de::Visitor<'_> for Visitor<N> {
            type Value = Text<N>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "at most {N} UTF-8 bytes")
            }
            fn visit_str<E: de::Error>(self, text: &str) -> Result<Self::Value, E> {
                if text.len() > N {
                    return Err(E::custom("text bound exceeded"));
                }
                Ok(Text(text.to_owned()))
            }
        }
        // postcard lends a slice of the already bounded input packet here.
        deserializer.deserialize_str(Visitor::<N>)
    }
}
