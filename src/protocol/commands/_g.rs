#[derive(PartialEq, Eq, Debug)]
pub struct g;

impl g {
    pub fn parse(body: &str) -> Result<Self, ()> {
        if !body.is_empty() {
            return Err(());
        }
        Ok(g)
    }
}
