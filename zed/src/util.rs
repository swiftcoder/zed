use postage::prelude::Stream;
use rand::prelude::*;
use std::{cmp::Ordering, future::Future, sync::Arc};
use zed_rpc::{proto, Peer, TypedEnvelope};

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum Bias {
    Left,
    Right,
}

impl PartialOrd for Bias {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Bias {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Left, Self::Left) => Ordering::Equal,
            (Self::Left, Self::Right) => Ordering::Less,
            (Self::Right, Self::Right) => Ordering::Equal,
            (Self::Right, Self::Left) => Ordering::Greater,
        }
    }
}

pub fn post_inc(value: &mut usize) -> usize {
    let prev = *value;
    *value += 1;
    prev
}

/// Extend a sorted vector with a sorted sequence of items, maintaining the vector's sort order and
/// enforcing a maximum length. Sort the items according to the given callback. Before calling this,
/// both `vec` and `new_items` should already be sorted according to the `cmp` comparator.
pub fn extend_sorted<T, I, F>(vec: &mut Vec<T>, new_items: I, limit: usize, mut cmp: F)
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T, &T) -> Ordering,
{
    let mut start_index = 0;
    for new_item in new_items {
        if let Err(i) = vec[start_index..].binary_search_by(|m| cmp(m, &new_item)) {
            let index = start_index + i;
            if vec.len() < limit {
                vec.insert(index, new_item);
            } else if index < vec.len() {
                vec.pop();
                vec.insert(index, new_item);
            }
            start_index = index;
        }
    }
}

pub trait MessageHandler<'a, M: proto::EnvelopedMessage> {
    type Output: 'a + Future<Output = anyhow::Result<()>>;

    fn handle(
        &self,
        message: TypedEnvelope<M>,
        rpc: Arc<Peer>,
        cx: &'a mut gpui::AsyncAppContext,
    ) -> Self::Output;
}

impl<'a, M, F, Fut> MessageHandler<'a, M> for F
where
    M: proto::EnvelopedMessage,
    F: Fn(TypedEnvelope<M>, Arc<Peer>, &'a mut gpui::AsyncAppContext) -> Fut,
    Fut: 'a + Future<Output = anyhow::Result<()>>,
{
    type Output = Fut;

    fn handle(
        &self,
        message: TypedEnvelope<M>,
        rpc: Arc<Peer>,
        cx: &'a mut gpui::AsyncAppContext,
    ) -> Self::Output {
        (self)(message, rpc, cx)
    }
}

pub fn handle_messages<H, M>(handler: H, rpc: &Arc<Peer>, cx: &mut gpui::MutableAppContext)
where
    H: 'static + for<'a> MessageHandler<'a, M>,
    M: proto::EnvelopedMessage,
{
    let rpc = rpc.clone();
    let mut messages = smol::block_on(rpc.add_message_handler::<M>());
    cx.spawn(|mut cx| async move {
        while let Some(message) = messages.recv().await {
            if let Err(err) = handler.handle(message, rpc.clone(), &mut cx).await {
                log::error!("error handling message: {:?}", err);
            }
        }
    })
    .detach();
}

pub struct RandomCharIter<T: Rng>(T);

impl<T: Rng> RandomCharIter<T> {
    #[cfg(test)]
    pub fn new(rng: T) -> Self {
        Self(rng)
    }
}

impl<T: Rng> Iterator for RandomCharIter<T> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.gen_bool(1.0 / 5.0) {
            Some('\n')
        }
        // two-byte greek letters
        else if self.0.gen_bool(1.0 / 8.0) {
            Some(std::char::from_u32(self.0.gen_range(('α' as u32)..('ω' as u32 + 1))).unwrap())
        }
        // three-byte characters
        else if self.0.gen_bool(1.0 / 10.0) {
            ['✋', '✅', '❌', '❎', '⭐'].choose(&mut self.0).cloned()
        }
        // four-byte characters
        else if self.0.gen_bool(1.0 / 12.0) {
            ['🍐', '🏀', '🍗', '🎉'].choose(&mut self.0).cloned()
        }
        // ascii letters
        else {
            Some(self.0.gen_range(b'a'..b'z' + 1).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extend_sorted() {
        let mut vec = vec![];

        extend_sorted(&mut vec, vec![21, 17, 13, 8, 1, 0], 5, |a, b| b.cmp(a));
        assert_eq!(vec, &[21, 17, 13, 8, 1]);

        extend_sorted(&mut vec, vec![101, 19, 17, 8, 2], 8, |a, b| b.cmp(a));
        assert_eq!(vec, &[101, 21, 19, 17, 13, 8, 2, 1]);

        extend_sorted(&mut vec, vec![1000, 19, 17, 9, 5], 8, |a, b| b.cmp(a));
        assert_eq!(vec, &[1000, 101, 21, 19, 17, 13, 9, 8]);
    }
}

// Allow surf Results to accept context like other Results do when
// using anyhow.
pub trait SurfResultExt {
    fn context<C>(self, cx: C) -> Self
    where
        C: std::fmt::Display + Send + Sync + 'static;

    fn with_context<C, F>(self, f: F) -> Self
    where
        C: std::fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> C;
}

impl<T> SurfResultExt for surf::Result<T> {
    fn context<C>(self, cx: C) -> Self
    where
        C: std::fmt::Display + Send + Sync + 'static,
    {
        self.map_err(|e| surf::Error::new(e.status(), e.into_inner().context(cx)))
    }

    fn with_context<C, F>(self, f: F) -> Self
    where
        C: std::fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> C,
    {
        self.map_err(|e| surf::Error::new(e.status(), e.into_inner().context(f())))
    }
}
