mod connector;
mod external;
mod types;

pub type Connector = connector::Connector<external::ZammadClient>;
