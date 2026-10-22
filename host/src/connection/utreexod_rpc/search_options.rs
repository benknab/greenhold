/// Pagination and response options for address history.
#[derive(Clone, Copy, Debug)]
pub struct SearchOptions {
    pub skip: u32,
    pub count: u32,
    pub input_details: InputDetails,
    pub order: HistoryOrder,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            skip: 0,
            count: 100,
            input_details: InputDetails::Omit,
            order: HistoryOrder::OldestFirst,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputDetails {
    Omit,
    Include,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryOrder {
    OldestFirst,
    NewestFirst,
}
