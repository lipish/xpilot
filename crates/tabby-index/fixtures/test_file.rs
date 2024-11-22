use std::sync::Arc;

pub struct Cache {
    inner: Arc<Inner>,
}

struct Inner {
    data: Vec<String>,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                data: Vec::new(),
            }),
        }
    }

    pub fn add(&self, item: String) {
        let mut data = self.inner.data.clone();
        data.push(item);
    }
}
