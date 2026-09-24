/// Generates 10 random alphanumeric characters.
pub fn get_short_id() -> String {
    use rand::distr::SampleString;
    rand::distr::Alphanumeric.sample_string(&mut rand::rng(), 10)
}
