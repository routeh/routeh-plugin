//! Reading a TOML table key by key, every refusal at the path of its key,
//! and any key that was not read refused as unknown.
use crate::ManifestError;
use toml::{Table, Value};

/// A table being read at `at` (`tunnel.settings.jc`; empty for the
/// manifest itself).
pub(crate) struct Section<'a> {
    at: String,
    table: &'a Table,
    taken: Vec<&'a str>,
}

impl<'a> Section<'a> {
    pub(crate) fn new(at: impl Into<String>, table: &'a Table) -> Self {
        Self {
            at: at.into(),
            table,
            taken: Vec::new(),
        }
    }

    /// The path of `key` in this table.
    pub(crate) fn path(&self, key: &str) -> String {
        if self.at.is_empty() {
            key.to_owned()
        } else {
            format!("{}.{key}", self.at)
        }
    }

    /// A refusal at `key`.
    pub(crate) fn refuse<T>(
        &self,
        key: &str,
        reason: impl Into<String>,
    ) -> Result<T, ManifestError> {
        Err(ManifestError::new(self.path(key), reason))
    }

    /// A refusal at the table itself.
    pub(crate) fn refuse_here<T>(&self, reason: impl Into<String>) -> Result<T, ManifestError> {
        Err(ManifestError::new(self.at.clone(), reason))
    }

    pub(crate) fn get(&mut self, key: &'a str) -> Option<&'a Value> {
        let (name, value) = self.table.get_key_value(key)?;
        self.taken.push(name.as_str());
        Some(value)
    }

    pub(crate) fn required<T>(
        &mut self,
        key: &'a str,
        read: impl FnOnce(&mut Self, &'a str) -> Result<Option<T>, ManifestError>,
    ) -> Result<T, ManifestError> {
        match read(self, key)? {
            Some(value) => Ok(value),
            None => self.refuse(key, "required"),
        }
    }

    pub(crate) fn string(&mut self, key: &'a str) -> Result<Option<&'a str>, ManifestError> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::String(text)) => Ok(Some(text)),
            Some(_) => self.refuse(key, "must be a string"),
        }
    }

    pub(crate) fn integer(&mut self, key: &'a str) -> Result<Option<i64>, ManifestError> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Integer(number)) => Ok(Some(*number)),
            Some(_) => self.refuse(key, "must be an integer"),
        }
    }

    /// An integer within `low..=high`, refused as `low to high`.
    pub(crate) fn bounded(
        &mut self,
        key: &'a str,
        low: i64,
        high: i64,
    ) -> Result<Option<i64>, ManifestError> {
        match self.integer(key)? {
            Some(number) if !(low..=high).contains(&number) => {
                self.refuse(key, format!("{low} to {high}"))
            }
            number => Ok(number),
        }
    }

    pub(crate) fn boolean(&mut self, key: &'a str) -> Result<Option<bool>, ManifestError> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Boolean(flag)) => Ok(Some(*flag)),
            Some(_) => self.refuse(key, "must be a boolean"),
        }
    }

    pub(crate) fn strings(&mut self, key: &'a str) -> Result<Option<Vec<&'a str>>, ManifestError> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Value::String(text) => Ok(text.as_str()),
                    _ => self.refuse(key, "must be an array of strings"),
                })
                .collect::<Result<_, _>>()
                .map(Some),
            Some(_) => self.refuse(key, "must be an array of strings"),
        }
    }

    pub(crate) fn table(&mut self, key: &'a str) -> Result<Option<Section<'a>>, ManifestError> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Table(table)) => Ok(Some(Section::new(self.path(key), table))),
            Some(_) => self.refuse(key, "must be a table"),
        }
    }

    /// An array of tables (`[[pages]]`); each is read at the same path.
    pub(crate) fn tables(&mut self, key: &'a str) -> Result<Vec<Section<'a>>, ManifestError> {
        match self.get(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Value::Table(table) => Ok(Section::new(self.path(key), table)),
                    _ => self.refuse(key, "must be an array of tables"),
                })
                .collect(),
            Some(_) => self.refuse(key, "must be an array of tables"),
        }
    }

    /// The keys of the table, in order, with their values; all of them
    /// count as read.
    pub(crate) fn entries(&mut self) -> Vec<(&'a str, &'a Value)> {
        let entries: Vec<_> = self
            .table
            .iter()
            .map(|(key, value)| (key.as_str(), value))
            .collect();
        self.taken.extend(entries.iter().map(|(key, _)| *key));
        entries
    }

    /// Refuses the first key, in order, that was not read.
    pub(crate) fn finish(self) -> Result<(), ManifestError> {
        match self
            .table
            .keys()
            .find(|key| !self.taken.contains(&key.as_str()))
        {
            Some(key) => self.refuse(key, "unknown key"),
            None => Ok(()),
        }
    }
}

/// Every item of `items` once, else the first named again.
pub(crate) fn distinct<'a>(items: impl IntoIterator<Item = &'a str>) -> Result<(), &'a str> {
    let mut seen = Vec::new();
    for item in items {
        if seen.contains(&item) {
            return Err(item);
        }
        seen.push(item);
    }
    Ok(())
}
