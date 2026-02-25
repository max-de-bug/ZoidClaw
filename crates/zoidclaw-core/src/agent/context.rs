//! Context builder for assembling agent prompts.
//!
//! Assembles the system prompt from identity, bootstrap files, memory,
//! skills, and conversation history into a coherent prompt for the LLM.

use std::path::Path;

use crate::agent::memory::MemoryStore;
use crate::agent::skills::SkillsLoader;
use crate::provider::types::ChatMessage;

/// Builds the context (system prompt + messages) for the agent.
pub struct ContextBuilder<'a> {
    workspace: &'a Path,
    memory: &'a MemoryStore,
    skills: &'a SkillsLoader,
    channel: String,
    chat_id: String,
    service_status: String,
}

impl<'a> ContextBuilder<'a> {
    pub fn new(
        workspace: &'a Path,
        memory: &'a MemoryStore,
        skills: &'a SkillsLoader,
        channel: &str,
        chat_id: &str,
        service_status: &str,
    ) -> Self {
        Self {
            workspace,
            memory,
            skills,
            channel: channel.to_string(),
            chat_id: chat_id.to_string(),
            service_status: service_status.to_string(),
        }
    }

    /// Build the complete system prompt.
    pub fn build_system_prompt(&self, skill_names: &[String]) -> String {
        let mut sections = Vec::new();

        // 1. Core identity
        sections.push(self.identity());

        // 2. Bootstrap files (workspace/SYSTEM.md, etc.)
        if let Some(bootstrap) = self.load_bootstrap_files() {
            sections.push(bootstrap);
        }

        // 3. Memory context
        let memory_ctx = self.memory.context();
        if !memory_ctx.is_empty() {
            sections.push(format!("# Memory\n\n{}", memory_ctx));
        }

        // 4. Skills
        if !skill_names.is_empty() {
            let skills_content = self.skills.load_skills_for_context(skill_names);
            if !skills_content.is_empty() {
                sections.push(skills_content);
            }
        }

        // 5. Skills summary (for progressive loading)
        let summary = self.skills.build_summary();
        if !summary.is_empty() {
            sections.push(summary);
        }

        sections.join("\n\n")
    }

    /// Build the complete message list for an LLM call.
    pub fn build_messages(
        &self,
        history: &[ChatMessage],
        current_message: &str,
        skill_names: &[String],
    ) -> Vec<ChatMessage> {
        let system_prompt = self.build_system_prompt(skill_names);
        let mut messages = vec![ChatMessage::system(&system_prompt)];

        // Add conversation history
        messages.extend_from_slice(history);

        // Add current user message
        messages.push(ChatMessage::user(current_message));

        messages
    }

    /// Add a tool result to the message list.
    pub fn add_tool_result(
        messages: &mut Vec<ChatMessage>,
        tool_call_id: &str,
        tool_name: &str,
        result: &str,
    ) {
        messages.push(ChatMessage::tool_result(tool_call_id, tool_name, result));
    }

    /// Add an assistant message with tool calls to the message list.
    pub fn add_assistant_tool_calls(
        messages: &mut Vec<ChatMessage>,
        content: Option<&str>,
        tool_calls: Vec<crate::provider::types::ToolCallMessage>,
    ) {
        messages.push(ChatMessage::assistant_with_tool_calls(content, tool_calls));
    }

    // ── Private helpers ─────────────────────────────────────────────

    fn identity(&self) -> String {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        format!(
            r#"# Identity

You are **zoidclaw** 🦀, an ultra-lightweight personal AI assistant.

## Environment (LIVE STATUS - ALWAYS TRUST THIS OVER MEMORY)
- Workspace: `{}`
- Channel: `{}`
- Chat ID: `{}`
- Service Status: {}
- Current time: {}
- Platform: {} ({})

## Capabilities
You have access to tools for:
- Reading, writing, and editing files
- Executing shell commands
- Searching the web and fetching web pages
- Managing scheduled tasks (cron)

## Guidelines
- Be concise, accurate, and helpful.
- Use tools when needed — don't guess about file contents or command outputs.
- When making changes to files, show what you changed.
- If unsure, ask for clarification.
- Prefer simple, correct solutions over clever ones."#,
            self.workspace.display(),
            self.channel,
            self.chat_id,
            self.service_status,
            timestamp,
            os,
            arch,
        )
    }

    fn load_bootstrap_files(&self) -> Option<String> {
        let candidates = ["SYSTEM.md", "CLAUDE.md", "INSTRUCTIONS.md"];
        let mut parts = Vec::new();

        for filename in &candidates {
            let path = self.workspace.join(filename);
            if let Ok(content) = std::fs::read_to_string(&path) {
                parts.push(format!("## {}\n\n{}", filename, content.trim()));
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(format!("# Bootstrap\n\n{}", parts.join("\n\n")))
        }
    }
}
