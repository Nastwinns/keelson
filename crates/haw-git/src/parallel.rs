//! Bounded fan-out across repos with plain scoped threads.
//! (tokio arrives with the async forge APIs; process-spawning git work
//! doesn't need an async runtime.)

/// Run `f` over `items` with at most `jobs` worker threads.
/// Results come back in input order. Panics in `f` propagate.
pub fn fan_out<T, R, F>(items: &[T], jobs: usize, f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    if items.is_empty() {
        return Vec::new();
    }
    let jobs = jobs.clamp(1, items.len());
    let f = &f;

    let mut slots: Vec<Option<R>> = std::iter::repeat_with(|| None).take(items.len()).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..jobs)
            .map(|worker| {
                scope.spawn(move || {
                    let mut out = Vec::new();
                    let mut index = worker;
                    while index < items.len() {
                        out.push((index, f(&items[index])));
                        index += jobs;
                    }
                    out
                })
            })
            .collect();
        for handle in handles {
            match handle.join() {
                Ok(results) => {
                    for (index, result) in results {
                        slots[index] = Some(result);
                    }
                }
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
    });
    slots
        .into_iter()
        .map(|slot| match slot {
            Some(result) => result,
            None => unreachable!("strided fan-out fills every slot"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::fan_out;

    #[test]
    fn preserves_input_order() {
        let items: Vec<u64> = (0..37).collect();
        let doubled = fan_out(&items, 4, |n| n * 2);
        assert_eq!(doubled, items.iter().map(|n| n * 2).collect::<Vec<_>>());
    }

    #[test]
    fn handles_empty_and_oversized_jobs() {
        assert!(fan_out::<u8, u8, _>(&[], 8, |n| *n).is_empty());
        assert_eq!(fan_out(&[1], 64, |n| n + 1), vec![2]);
    }
}
