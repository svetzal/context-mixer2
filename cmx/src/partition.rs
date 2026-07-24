//! Batch classification of artifact names during adoption/partitioning.

use crate::error::Result;

/// Outcome of classifying a single name during batch partition.
pub enum Partitioned<S, E> {
    /// Name was accepted; its result is carried as `S`.
    Kept(S),
    /// Name was excluded; its reason is carried as `E`.
    Excluded(E),
}

/// Classify each name via `classify`, collecting `Kept` outcomes into the first
/// vec and `Excluded` outcomes into the second. Propagates `Err` with `?`.
pub fn partition_by<S, E, F>(names: &[String], mut classify: F) -> Result<(Vec<S>, Vec<E>)>
where
    F: FnMut(&str) -> Result<Partitioned<S, E>>,
{
    let mut kept = Vec::new();
    let mut excluded = Vec::new();
    for name in names {
        match classify(name)? {
            Partitioned::Kept(s) => kept.push(s),
            Partitioned::Excluded(e) => excluded.push(e),
        }
    }
    Ok((kept, excluded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_by_all_kept() {
        let names: Vec<String> = vec!["a".to_string(), "b".to_string()];
        let (kept, excluded): (Vec<String>, Vec<String>) =
            partition_by(&names, |name| Ok(Partitioned::Kept(name.to_string()))).unwrap();
        assert_eq!(kept, vec!["a".to_string(), "b".to_string()]);
        assert!(excluded.is_empty());
    }

    #[test]
    fn partition_by_mixed_kept_and_excluded() {
        let names: Vec<String> = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let (kept, excluded): (Vec<String>, Vec<String>) = partition_by(&names, |name| {
            if name == "b" {
                Ok(Partitioned::Excluded(name.to_string()))
            } else {
                Ok(Partitioned::Kept(name.to_string()))
            }
        })
        .unwrap();
        assert_eq!(kept, vec!["a".to_string(), "c".to_string()]);
        assert_eq!(excluded, vec!["b".to_string()]);
    }

    #[test]
    fn partition_by_propagates_err() {
        let names: Vec<String> = vec!["ok".to_string(), "boom".to_string()];
        let result: Result<(Vec<String>, Vec<String>)> = partition_by(&names, |name| {
            if name == "boom" {
                return Err(crate::error::CliError::Message("hard error".to_string()));
            }
            Ok(Partitioned::Kept(name.to_string()))
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("hard error"));
    }
}
