//! Parallel planner — deterministic ordering with bounded concurrency.
//!
//! Mirrors `termproof/cli.py`'s `ThreadPoolExecutor` usage but exposes a
//! reusable planner that is race-safe and order-preserving.

use std::path::PathBuf;

/// One unit of work: a recipe file × renderer.
#[derive(Debug, Clone)]
pub struct PlanItem {
    /// Recipe file path.
    pub recipe_path: PathBuf,
    /// Recipe name (sanitized for display).
    pub recipe_name: String,
    /// Renderer name.
    pub renderer: String,
    /// Extra argv for the renderer.
    pub renderer_argv: Vec<String>,
}

/// Expand a list of recipe paths × renderers into a deterministic plan.
///
/// The output is sorted by `(recipe_name, renderer)` so parallel execution
/// remains reproducible regardless of filesystem ordering.
#[allow(clippy::type_complexity)]
pub fn plan_items(recipes: Vec<(PathBuf, String, Vec<(String, Vec<String>)>)>) -> Vec<PlanItem> {
    // recipes: Vec<(path, recipe_name, renderers)>
    let mut items = Vec::new();
    for (path, name, renderers) in recipes {
        for (renderer, argv) in renderers {
            items.push(PlanItem {
                recipe_path: path.clone(),
                recipe_name: name.clone(),
                renderer,
                renderer_argv: argv,
            });
        }
    }
    items.sort_by(|a, b| {
        a.recipe_name
            .cmp(&b.recipe_name)
            .then_with(|| a.renderer.cmp(&b.renderer))
    });
    items
}

/// Execute `items` in parallel with bounded workers, preserving input order in
/// the output.
///
/// `f` is called once per item; its return values are collected in plan order
/// (like `ThreadPoolExecutor.map`).  Panics in `f` propagate as errors.
pub fn run_parallel<T, F>(items: Vec<PlanItem>, max_workers: usize, f: F) -> Vec<T>
where
    F: Fn(&PlanItem) -> T + Send + Sync,
    T: Send,
{
    if max_workers <= 1 || items.len() <= 1 {
        return items.iter().map(&f).collect();
    }
    // Use std::thread scope for simplicity (no extra deps).
    let chunk_size = items.len().div_ceil(max_workers);
    let f_ref = &f;
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in items.chunks(chunk_size) {
            let chunk_vec: Vec<PlanItem> = chunk.to_vec();
            let handle = scope.spawn(move || {
                let mut out = Vec::with_capacity(chunk_vec.len());
                for item in &chunk_vec {
                    out.push(f_ref(item));
                }
                out
            });
            handles.push(handle);
        }
        // Collect in chunk order to preserve plan ordering.
        let mut results = Vec::with_capacity(items.len());
        for h in handles {
            results.extend(h.join().expect("planner worker panicked"));
        }
        results
    })
}
