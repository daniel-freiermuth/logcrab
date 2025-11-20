# Architecture Guide - Adding Embeddings (v2)

## Current Architecture (v1)

The scoring system is built on a flexible trait-based design:

```rust
pub trait AnomalyScorer: Send {
    fn score(&mut self, line: &LogLine) -> f64;  // Score before updating
    fn update(&mut self, line: &LogLine);         // Update state after scoring
    fn reset(&mut self);                          // Reset state
}
```

## How to Add Embeddings (Example)

Here's how you would extend the system with embedding-based scoring:

### Step 1: Add Dependencies

```toml
# Cargo.toml
[dependencies]
# ... existing dependencies ...
tokenizers = "0.15"        # For text tokenization
ort = "2.0"                # ONNX runtime for embeddings
ndarray = "0.15"           # Array operations
```

### Step 2: Create EmbeddingScorer

```rust
// src/anomaly/embedding.rs

use crate::parser::line::LogLine;
use crate::anomaly::scorer::AnomalyScorer;
use ndarray::{Array1, Array2};
use std::collections::VecDeque;

pub struct EmbeddingScorer {
    model: EmbeddingModel,              // Your embedding model
    embedding_cache: Vec<Array1<f32>>,  // Past embeddings
    window_size: usize,
}

impl EmbeddingScorer {
    pub fn new(model_path: &str, window_size: usize) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(EmbeddingScorer {
            model: EmbeddingModel::load(model_path)?,
            embedding_cache: Vec::new(),
            window_size,
        })
    }
    
    fn compute_embedding(&self, text: &str) -> Array1<f32> {
        // Compute embedding using your model
        // This would call into ONNX runtime, sentence-transformers, etc.
        self.model.encode(text)
    }
    
    fn compute_novelty(&self, embedding: &Array1<f32>) -> f64 {
        if self.embedding_cache.is_empty() {
            return 1.0;
        }
        
        // Compute cosine similarity with all cached embeddings
        let similarities: Vec<f64> = self.embedding_cache
            .iter()
            .map(|cached| cosine_similarity(embedding, cached))
            .collect();
        
        // Novel = low max similarity
        let max_similarity = similarities.iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        
        1.0 - max_similarity
    }
}

impl AnomalyScorer for EmbeddingScorer {
    fn score(&mut self, line: &LogLine) -> f64 {
        let embedding = self.compute_embedding(&line.message);
        self.compute_novelty(&embedding)
    }
    
    fn update(&mut self, line: &LogLine) {
        let embedding = self.compute_embedding(&line.message);
        
        if self.embedding_cache.len() >= self.window_size {
            self.embedding_cache.remove(0);
        }
        
        self.embedding_cache.push(embedding);
    }
    
    fn reset(&mut self) {
        self.embedding_cache.clear();
    }
}

fn cosine_similarity(a: &Array1<f32>, b: &Array1<f32>) -> f64 {
    let dot = a.dot(b);
    let norm_a = a.dot(a).sqrt();
    let norm_b = b.dot(b).sqrt();
    (dot / (norm_a * norm_b)) as f64
}
```

### Step 3: Integrate into CompositeScorer

```rust
// src/anomaly/mod.rs

pub mod embedding;  // Add this

use embedding::EmbeddingScorer;

pub fn create_default_scorer() -> CompositeScorer {
    CompositeScorer::new()
        .add_scorer(Box::new(RarityScorer::new()), 3.0)
        .add_scorer(Box::new(TemporalScorer::new(30)), 2.0)
        .add_scorer(Box::new(EntropyScorer::new()), 1.5)
        .add_scorer(Box::new(SeverityScorer::new(100)), 2.5)
}

pub fn create_embedding_scorer(model_path: &str) -> Result<CompositeScorer, Box<dyn std::error::Error>> {
    Ok(CompositeScorer::new()
        .add_scorer(Box::new(RarityScorer::new()), 2.0)          // Reduce weight
        .add_scorer(Box::new(TemporalScorer::new(30)), 1.5)      // Reduce weight
        .add_scorer(Box::new(EntropyScorer::new()), 1.0)         // Reduce weight
        .add_scorer(Box::new(SeverityScorer::new(100)), 2.0)     // Keep severity important
        .add_scorer(Box::new(EmbeddingScorer::new(model_path, 1000)?), 4.0))  // High weight!
}
```

### Step 4: Update App to Support Model Selection

```rust
// src/app.rs

pub struct LogOwlApp {
    log_view: LogView,
    use_embeddings: bool,
    model_path: Option<PathBuf>,
    // ... rest of fields
}

impl LogOwlApp {
    fn process_file(&mut self, path: &PathBuf) -> Result<Vec<LogLine>, std::io::Error> {
        // ... existing code ...
        
        let mut scorer = if self.use_embeddings {
            if let Some(ref model_path) = self.model_path {
                match create_embedding_scorer(model_path.to_str().unwrap()) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Failed to load embedding model: {}", e);
                        create_default_scorer()
                    }
                }
            } else {
                create_default_scorer()
            }
        } else {
            create_default_scorer()
        };
        
        // ... rest of processing ...
    }
}
```

### Step 5: Add UI Controls

```rust
// src/app.rs - in update() method

egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
    egui::menu::bar(ui, |ui| {
        ui.menu_button("File", |ui| {
            // ... existing menu items ...
        });
        
        ui.menu_button("Options", |ui| {
            if ui.checkbox(&mut self.use_embeddings, "Use Embedding Scorer").clicked() {
                if self.use_embeddings && self.model_path.is_none() {
                    // Prompt for model path
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ONNX Model", &["onnx"])
                        .pick_file() 
                    {
                        self.model_path = Some(path);
                    }
                }
            }
        });
    });
});
```

## Benefits of This Architecture

1. **Zero Breaking Changes**: Existing scorers continue to work
2. **Easy Testing**: Test embedding scorer independently
3. **Flexible Composition**: Mix and match scorers with different weights
4. **Performance**: Can add caching, batching within the scorer
5. **Model Agnostic**: Use any embedding model (BERT, Sentence-BERT, OpenAI, etc.)

## Alternative: Hybrid Approach

You could also create a "smart" scorer that uses embeddings only for uncertain cases:

```rust
pub struct HybridScorer {
    fast_scorers: CompositeScorer,
    embedding_scorer: EmbeddingScorer,
    uncertainty_threshold: f64,
}

impl AnomalyScorer for HybridScorer {
    fn score(&mut self, line: &LogLine) -> f64 {
        let fast_score = self.fast_scorers.score(line);
        
        // Only use expensive embeddings when fast methods are uncertain
        if fast_score > 0.4 && fast_score < 0.6 {
            let embedding_score = self.embedding_scorer.score(line);
            (fast_score + embedding_score) / 2.0
        } else {
            fast_score
        }
    }
    
    // ... rest of implementation
}
```

This gives you the best of both worlds: fast processing for obvious cases, deep analysis for edge cases.

## Performance Considerations

1. **Batch Embeddings**: Process multiple messages at once
2. **Cache Results**: Store embeddings by template key
3. **Quantize Model**: Use int8 quantization for speed
4. **GPU Acceleration**: Use CUDA/Metal if available
5. **Lazy Loading**: Load model only when needed

## Example Models to Use

- **Sentence-BERT** (all-MiniLM-L6-v2): Fast, good quality
- **Universal Sentence Encoder**: Google's general-purpose encoder  
- **RoBERTa**: Better for code/technical logs
- **Domain-specific**: Fine-tune on your own logs

The architecture is ready - just plug in the embedding scorer when you're ready for v2! 🚀
