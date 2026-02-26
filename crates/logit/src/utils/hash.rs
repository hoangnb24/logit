use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[must_use]
pub fn hash64<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::hash64;

    #[test]
    fn hash_is_stable_for_same_input() {
        let value = "logit-stable";
        assert_eq!(hash64(&value), hash64(&value));
    }

    #[test]
    fn hash_differs_for_different_inputs() {
        assert_ne!(hash64(&"alpha"), hash64(&"beta"));
    }
}
