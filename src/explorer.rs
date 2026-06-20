pub struct Explorer {
    name: String
}

impl Explorer {
    pub fn new(name: String) -> Self {
        Explorer { name }
    }
}

pub struct BagItem {
    name: String,
    amount: u32
}