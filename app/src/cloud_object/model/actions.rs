use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::server::ids::{HashedSqliteId, ObjectUid};

/// The type of action that occurred on an object, such as an execution, selection, so on
/// and so forth.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectActionType {
    Execute,
}

// In order to convert from a graphql type and from a SQLite read, the action type
// implements to_string().
//
// Temporarily suppress clippy warnings about the `ToString` impl until we
// move `ObjectType` away from using `std::fmt::Display` for serialization.
#[allow(clippy::to_string_trait_impl)]
impl ToString for ObjectActionType {
    fn to_string(&self) -> String {
        match self {
            ObjectActionType::Execute => String::from("EXECUTE"),
        }
    }
}

impl ObjectActionType {
    pub fn singular(&self) -> String {
        match self {
            ObjectActionType::Execute => "run".to_string(),
        }
    }

    pub fn plural(&self) -> String {
        match self {
            ObjectActionType::Execute => "runs".to_string(),
        }
    }
}

/// We track object actions, both those that have been sent to the server and not, through this
/// type. A single ObjectAction represents an object_id, action pair and a subtype that contains data
/// about the action(s). Each ObjectAction either represents one action or a summary of identical actions
/// that occurred at different times. We summarize old actions in order to save memory footprint on the client.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectAction {
    pub action_type: ObjectActionType,
    pub uid: ObjectUid,
    pub hashed_sqlite_id: HashedSqliteId,
    // This action either represents one action or a consolidation of multiple actions.
    pub action_subtype: ObjectActionSubtype,
}

impl ObjectAction {
    pub fn is_pending(&self) -> bool {
        match self.action_subtype {
            ObjectActionSubtype::SingleAction { pending, .. } => pending,
            ObjectActionSubtype::BundledActions { .. } => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectActionSubtype {
    SingleAction {
        timestamp: DateTime<Utc>,
        processed_at_timestamp: Option<DateTime<Utc>>,
        data: Option<String>,
        pending: bool,
    },
    BundledActions {
        count: i32,
        oldest_timestamp: DateTime<Utc>,
        latest_timestamp: DateTime<Utc>,
        latest_processed_at_timestamp: DateTime<Utc>,
    },
}

pub enum ObjectActionsEvent {}

#[cfg(not(target_family = "wasm"))]
pub fn object_action_from_persisted(
    other: crate::persistence::model::PersistedObjectAction,
) -> Result<ObjectAction, ()> {
    // Each persisted object action is either a single action or a bundled action.
    // If there's any inconsistencies from the SQL row, we return an error.
    let action_subtype = if let Some(count) = other.count {
        let oldest_timestamp = other
            .oldest_timestamp
            .as_ref()
            .map(|time| time.and_utc())
            .ok_or(())?;
        let latest_timestamp = other
            .latest_timestamp
            .as_ref()
            .map(|time| time.and_utc())
            .ok_or(())?;

        // When the db row is a bundled action, the processed_at_timestamp field refers
        // to the latest processed_at_timestamp in the bundle. Because bundled actions come
        // from the server, this is a value, not an option.
        let latest_processed_at_timestamp = other
            .processed_at_timestamp
            .as_ref()
            .map(|time| time.and_utc())
            .ok_or(())?;
        ObjectActionSubtype::BundledActions {
            count,
            oldest_timestamp,
            latest_timestamp,
            latest_processed_at_timestamp,
        }
    } else {
        let timestamp = other
            .timestamp
            .as_ref()
            .map(|time| time.and_utc())
            .ok_or(())?;
        let pending = other.pending.ok_or(())?;

        // The processed_at_timestamp is still None when the action hasn't been synced.
        let processed_at_timestamp = other
            .processed_at_timestamp
            .as_ref()
            .map(|time| time.and_utc());
        ObjectActionSubtype::SingleAction {
            timestamp,
            data: other.data,
            pending,
            processed_at_timestamp,
        }
    };

    // The object_sync_id stored in SQLite is the hashed id that's used to index into the ObjectActions
    // model.
    let hashed_object_id = other.hashed_object_id;
    let action_type = match other.action.as_str() {
        s if s == ObjectActionType::Execute.to_string() => ObjectActionType::Execute,
        _ => return Err(()),
    };

    // NOTE: This is needed since we only store the sqlite hash, but we need the uid (the second part of the hash)
    // to index into CloudModel and store the object actions in memory.
    let uid = crate::server::ids::parse_sqlite_id_to_uid(hashed_object_id.clone())?;

    Ok(ObjectAction {
        uid: uid.to_string(),
        hashed_sqlite_id: hashed_object_id,
        action_type,
        action_subtype,
    })
}

/// A singleton model representing the actions that have occurred on a per-object basis. These
/// represent actions taken by the user or by teammates. The actions have a pending status that is
/// true when the server doesn't know about it and is false anytime after the action is successfully
/// synced.
pub struct ObjectActions {
    #[allow(dead_code)]
    object_actions_by_id: HashMap<ObjectUid, Vec<ObjectAction>>,
}

impl ObjectActions {
    /// Accepts a vector of object actions read out of SQLite.
    pub fn new(persisted_actions: Vec<ObjectAction>) -> Self {
        // Partitions the actions by object id and plops them into the map.
        let object_actions_by_id = persisted_actions.into_iter().fold(
            HashMap::new(),
            |mut map: HashMap<ObjectUid, Vec<ObjectAction>>, object_action| {
                map.entry(object_action.uid.clone())
                    .or_default()
                    .push(object_action);
                map
            },
        );

        Self {
            object_actions_by_id,
        }
    }

    /// Insert a single action into the model. Returns the created action.
    pub fn insert_action(
        &mut self,
        uid: ObjectUid,
        hashed_sqlite_id: HashedSqliteId,
        action_type: ObjectActionType,
        data: Option<String>,
        timestamp: DateTime<Utc>,
        ctx: &mut ModelContext<Self>,
    ) -> ObjectAction {
        // Create an action with pending=true.
        let action = ObjectAction {
            action_type,
            uid: uid.clone(),
            hashed_sqlite_id,
            action_subtype: ObjectActionSubtype::SingleAction {
                timestamp,
                data,
                pending: true,
                processed_at_timestamp: None,
            },
        };

        // Insert the action into the model.
        self.object_actions_by_id
            .entry(uid)
            .or_default()
            .push(action.clone());

        ctx.notify();

        action
    }

    /// Get the processed_at_timestamp of the most recent server-synced action we have for a given object. This determines
    /// whether or not we should accept some update from the server.
    pub fn get_latest_processed_at_ts(&self, uid: &ObjectUid) -> Option<DateTime<Utc>> {
        if let Some(actions) = self.object_actions_by_id.get(uid) {
            actions
                .iter()
                .filter_map(|a| match a.action_subtype {
                    ObjectActionSubtype::SingleAction {
                        processed_at_timestamp,
                        pending: false,
                        ..
                    } => processed_at_timestamp,
                    ObjectActionSubtype::BundledActions {
                        latest_processed_at_timestamp,
                        ..
                    } => Some(latest_processed_at_timestamp),
                    _ => None,
                })
                .max()
        } else {
            None
        }
    }

    /// Returns a time-boxed summary of the number of times this action type has occurred on this object.
    /// This summary prioritizes smaller units of time where possible, starting from Day and going to Year.
    /// If the action type has occurred on the object in the last day, we return "X actions in the last day".
    /// If not, we increase the time unit from Day to Week to Month. If no actions have occurred in the last month,
    /// we return however many actions have occurred in the last year, possibly 0.
    ///
    /// This function operates by cloning a filtered Iterator<Item=&ObjectAction>, saving some performance overhead
    /// by cloning references instead of objects.
    pub fn get_action_history_summary_for_action_type(
        &self,
        uid: &ObjectUid,
        action_type: ObjectActionType,
    ) -> Option<String> {
        // If the object is not in the model, return 0.
        let all_actions_on_this_object = self.object_actions_by_id.get(uid);
        if all_actions_on_this_object.is_none() {
            return Some("0 runs in the last year".to_string());
        }

        // If the object doesn't have any of these action types recorded, return 0.
        let all_relevant_actions = all_actions_on_this_object?
            .iter()
            .filter(|a| a.action_type == action_type);
        if all_relevant_actions.clone().count() == 0 {
            return Some("0 runs in the last year".to_string());
        }

        // If the action has occurred in the last day, return Day as the time unit.
        let one_day_ago = Utc::now() - Duration::days(1);
        let in_the_last_day = all_relevant_actions.clone().filter(|a| matches!(a.action_subtype, ObjectActionSubtype::SingleAction { timestamp, .. } if timestamp > one_day_ago)).count();
        if in_the_last_day > 0 {
            return Some(format!(
                "{} {} in the last day",
                in_the_last_day,
                if in_the_last_day == 1 {
                    action_type.singular()
                } else {
                    action_type.plural()
                }
            ));
        }

        // If the action has occurred in the last week, return Week as the time unit.
        let one_week_ago = Utc::now() - Duration::days(7);
        let in_the_last_week = all_relevant_actions.clone().filter(|a| matches!(a.action_subtype, ObjectActionSubtype::SingleAction { timestamp, .. } if timestamp > one_week_ago)).count();
        if in_the_last_week > 0 {
            return Some(format!(
                "{} {} in the last week",
                in_the_last_week,
                if in_the_last_week == 1 {
                    action_type.singular()
                } else {
                    action_type.plural()
                }
            ));
        }

        // If the action has occurred in the last month, return Month as the time unit.
        let one_month_ago = Utc::now() - Duration::days(30);
        let in_the_last_month = all_relevant_actions.clone().filter(|a| matches!(a.action_subtype, ObjectActionSubtype::SingleAction { timestamp, .. } if timestamp > one_month_ago)).count();
        if in_the_last_month > 0 {
            return Some(format!(
                "{} {} in the last month",
                in_the_last_month,
                if in_the_last_month == 1 {
                    action_type.singular()
                } else {
                    action_type.plural()
                }
            ));
        }

        // Finally, if all else turned up fruitless, return the yearly count.
        let one_year_ago = Utc::now() - Duration::days(365);
        let in_the_last_year: i32 = all_relevant_actions
            .clone()
            .filter_map(|a| match a.action_subtype {
                ObjectActionSubtype::SingleAction { timestamp, .. } if timestamp > one_year_ago => {
                    Some(1)
                }
                ObjectActionSubtype::BundledActions {
                    count,
                    oldest_timestamp,
                    ..
                } if oldest_timestamp > one_year_ago => Some(count),
                _ => None,
            })
            .sum();

        Some(format!(
            "{} {} in the last year",
            in_the_last_year,
            if in_the_last_year == 1 {
                action_type.singular()
            } else {
                action_type.plural()
            }
        ))
    }

    pub fn delete_actions_for_object(&mut self, uid: &ObjectUid, ctx: &mut ModelContext<Self>) {
        self.object_actions_by_id.remove(uid);
        ctx.notify()
    }
}

impl Entity for ObjectActions {
    type Event = ObjectActionsEvent;
}

impl SingletonEntity for ObjectActions {}

#[cfg(test)]
#[path = "actions_tests.rs"]
pub mod tests;
