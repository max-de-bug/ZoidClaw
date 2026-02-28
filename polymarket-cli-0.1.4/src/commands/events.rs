use anyhow::Result;
use clap::{Args, Subcommand};
use polymarket_client_sdk::gamma::{
    self,
    types::request::{EventByIdRequest, EventBySlugRequest, EventTagsRequest, EventsRequest},
};

use super::is_numeric_id;
use crate::output::events::{print_event_compact, print_event_detail, print_events_compact, print_events_table};
use crate::output::tags::{print_tags_compact, print_tags_table};
use crate::output::{OutputFormat, print_json};

#[derive(Args)]
pub struct EventsArgs {
    #[command(subcommand)]
    pub command: EventsCommand,
}

#[derive(Subcommand)]
pub enum EventsCommand {
    /// List events with optional filters
    List {
        /// Filter by active status
        #[arg(long)]
        active: Option<bool>,

        /// Filter by closed status
        #[arg(long)]
        closed: Option<bool>,

        /// Max results
        #[arg(long, default_value = "25")]
        limit: i32,

        /// Pagination offset
        #[arg(long)]
        offset: Option<i32>,

        /// Sort field (e.g. volume, liquidity, `created_at`)
        #[arg(long)]
        order: Option<String>,

        /// Sort ascending instead of descending
        #[arg(long)]
        ascending: bool,

        /// Filter by tag slug (e.g. "politics", "crypto")
        #[arg(long)]
        tag: Option<String>,
    },

    /// Get a single event by ID or slug
    Get {
        /// Event ID (numeric) or slug
        id: String,
    },

    /// Get tags for an event
    Tags {
        /// Event ID
        id: String,
    },
}

pub async fn execute(client: &gamma::Client, args: EventsArgs, output: OutputFormat) -> Result<()> {
    match args.command {
        EventsCommand::List {
            active,
            closed,
            limit,
            offset,
            order,
            ascending,
            tag,
        } => {
            // Default to active=true if neither active nor closed are specified
            let resolved_active = active.unwrap_or_else(|| closed.is_none() || closed == Some(false));
            let resolved_closed = closed.or_else(|| Some(!resolved_active));

            // Default to ordering by volume descending if not specified
            let resolved_order = order.unwrap_or_else(|| "volume".to_string());

            let request = EventsRequest::builder()
                .limit(limit)
                .maybe_closed(resolved_closed)
                .maybe_offset(offset)
                .maybe_ascending(if ascending { Some(true) } else { Some(false) })
                .maybe_tag_slug(tag)
                .order(vec![resolved_order])
                .build();

            let events = client.events(&request).await?;

            match output {
                OutputFormat::Table => print_events_table(&events),
                OutputFormat::Compact => print_events_compact(&events),
                OutputFormat::Json => print_json(&events)?,
            }
        }

        EventsCommand::Get { id } => {
            let is_numeric = is_numeric_id(&id);
            let event = if is_numeric {
                let req = EventByIdRequest::builder().id(id).build();
                client.event_by_id(&req).await?
            } else {
                let req = EventBySlugRequest::builder().slug(id).build();
                client.event_by_slug(&req).await?
            };

            match output {
                OutputFormat::Table => print_event_detail(&event),
                OutputFormat::Compact => print_event_compact(&event),
                OutputFormat::Json => print_json(&event)?,
            }
        }

        EventsCommand::Tags { id } => {
            let req = EventTagsRequest::builder().id(id).build();
            let tags = client.event_tags(&req).await?;

            match output {
                OutputFormat::Table => print_tags_table(&tags),
                OutputFormat::Compact => print_tags_compact(&tags),
                OutputFormat::Json => print_json(&tags)?,
            }
        }
    }

    Ok(())
}
