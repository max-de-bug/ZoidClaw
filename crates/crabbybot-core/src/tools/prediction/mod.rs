//! Prediction engine — MiroFish-inspired multi-agent simulation.
//!
//! This module provides a lightweight prediction engine that:
//! 1. Takes seed text (news, policies, etc.)
//! 2. Extracts an ontology of entity and relation types via LLM
//! 3. Builds a knowledge graph from the text
//! 4. Generates agent personas from graph entities
//! 5. Runs a multi-agent social simulation
//! 6. Produces a structured prediction report
//!
//! All components reuse CrabbyBot's existing [`LlmProvider`] trait
//! for LLM calls, keeping the prediction engine provider-agnostic.

pub mod graph;
pub mod graph_builder;
pub mod ontology;
pub mod profile_gen;
pub mod report;
pub mod simulation;
pub mod text_processor;
pub mod tool_graph_query;
pub mod tool_predict;
pub mod tool_simulate;
pub mod types;

pub use tool_graph_query::GraphQueryTool;
pub use tool_predict::PredictTool;
pub use tool_simulate::SimulateTool;
