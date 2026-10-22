use warpui::{Entity, ModelHandle};

use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::terminal::input::slash_commands::{
    AcceptSlashCommandOrSavedPrompt, SlashCommandDataSource,
};

pub struct ZeroStateDataSource {
    slash_command_data_source: ModelHandle<SlashCommandDataSource>,
}

impl ZeroStateDataSource {
    pub fn new(slash_command_data_source: &ModelHandle<SlashCommandDataSource>) -> Self {
        Self {
            slash_command_data_source: slash_command_data_source.clone(),
        }
    }
}

impl Entity for ZeroStateDataSource {
    type Event = ();
}

impl SyncDataSource for ZeroStateDataSource {
    type Action = AcceptSlashCommandOrSavedPrompt;

    fn run_query(
        &self,
        query: &Query,
        app: &warpui::AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        if !query.text.is_empty() {
            return Ok(vec![]);
        }

        let source = self.slash_command_data_source.as_ref(app);
        let results = source.ordered_zero_state_commands(app);

        Ok(results.into_iter().map(Into::into).collect())
    }
}
