pub struct Sms {
    from: String,
    to: String,
    content: String,
}

impl Sms {
    pub fn new() -> Sms {
        Sms {
            from: "".to_string(),
            to: "".to_string(),
            content: "".to_string(),
        }
    }

    pub fn from(&mut self, from: String) -> &mut Self {
        self.from = from;
        self
    }

    pub fn to(&mut self, to: String) -> &mut Self {
        self.to = to;
        self
    }

    pub fn content(&mut self, content: String) -> &mut Self {
        self.content = content;
        self
    }

    pub fn send(&self) {
        println!(
            "Sending SMS from {} to {} with content: {}",
            self.from, self.to, self.content
        );
    }
}
