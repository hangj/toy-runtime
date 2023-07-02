use std::sync::mpsc::Receiver;

pub struct Handle<T>
where
    T: Send + 'static,
{
    pub(crate) receiver: Receiver<T>,
}

impl<T> Handle<T>
where
    T: Send + 'static,
{
    pub(crate) fn from_receiver(rx: Receiver<T>) -> Self {
        Self { receiver: rx }
    }
}
