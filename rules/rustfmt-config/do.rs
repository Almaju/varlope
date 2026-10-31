use std::sync::Arc;

use parking_lot::Mutex;

use crate::enrollment::NotEnrolledReason;
use crate::stateful::enrollment::v3;
use crate::stateful::persistence::{Database, StoreId};

pub(crate) const DB_KEY_DB_VERSION: &str = "db_version";
