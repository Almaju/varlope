/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Row mapping for the logins table.

use rusqlite::Row;

pub(crate) fn guid(row: &Row<'_>) -> rusqlite::Result<String> {
    row.get("guid")
}
