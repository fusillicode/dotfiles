use chrono::DateTime;
use chrono::Utc;

#[derive(Clone)]
pub struct Footer {
    pub generated_at: DateTime<Utc>,
}
