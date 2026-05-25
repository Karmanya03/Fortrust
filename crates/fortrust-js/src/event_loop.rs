use std::cell::Cell;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use boa_engine::JsValue;
use tracing::{debug, trace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerKind {
    Timeout,
    Interval,
}

#[derive(Debug, Clone)]
pub struct TimerHandle {
    pub id: u64,
    pub kind: TimerKind,
    pub interval: Duration,
    pub created_at: Instant,
}

#[derive(Debug, Clone)]
struct TimerEntry {
    id: u64,
    fire_at: Instant,
    interval: Duration,
    kind: TimerKind,
    handler: JsValue,
}

#[derive(Debug, Clone)]
pub struct Macrotask {
    #[allow(dead_code)]
    pub id: u64,
    #[allow(dead_code)]
    pub handler: JsValue,
    #[allow(dead_code)]
    pub args: Vec<JsValue>,
}

#[derive(Debug, Clone)]
pub struct TaskQueue {
    pub microtasks: VecDeque<JsValue>,
    pub macrotasks: VecDeque<Macrotask>,
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskQueue {
    pub fn new() -> Self {
        Self {
            microtasks: VecDeque::new(),
            macrotasks: VecDeque::new(),
        }
    }

    pub fn enqueue_microtask(&mut self, task: JsValue) {
        self.microtasks.push_back(task);
    }

    pub fn enqueue_macrotask(&mut self, handler: JsValue, args: Vec<JsValue>) {
        static MACROTASK_ID: AtomicU64 = AtomicU64::new(1);
        let id = MACROTASK_ID.fetch_add(1, Ordering::Relaxed);
        self.macrotasks.push_back(Macrotask { id, handler, args });
    }

    pub fn drain_microtasks(&mut self) -> Vec<JsValue> {
        self.microtasks.drain(..).collect()
    }

    pub fn next_macrotask(&mut self) -> Option<Macrotask> {
        self.macrotasks.pop_front()
    }

    pub fn is_empty(&self) -> bool {
        self.microtasks.is_empty() && self.macrotasks.is_empty()
    }
}

#[derive(Clone)]
pub struct EventLoop {
    timers: Rc<RefCell<Vec<TimerEntry>>>,
    task_queue: Rc<RefCell<TaskQueue>>,
    next_timer_id: Rc<Cell<u64>>,
}

impl EventLoop {
    pub fn new() -> Self {
        Self {
            timers: Rc::new(RefCell::new(Vec::new())),
            task_queue: Rc::new(RefCell::new(TaskQueue::new())),
            next_timer_id: Rc::new(Cell::new(1)),
        }
    }

    pub fn schedule_timer(
        &self,
        id: u64,
        interval: Duration,
        kind: TimerKind,
        handler: JsValue,
    ) -> TimerHandle {
        let fire_at = Instant::now() + interval;
        self.timers.borrow_mut().push(TimerEntry {
            id,
            fire_at,
            interval,
            kind,
            handler: handler.clone(),
        });

        debug!(
            timer_id = id,
            kind = ?kind,
            delay_ms = interval.as_millis(),
            "Timer scheduled"
        );

        TimerHandle {
            id,
            kind,
            interval,
            created_at: Instant::now(),
        }
    }

    pub fn cancel_timer(&self, handle: TimerHandle) {
        self.timers
            .borrow_mut()
            .retain(|entry| entry.id != handle.id);
        debug!(timer_id = handle.id, "Timer cancelled");
    }

    pub fn process_pending_timers(&self) -> Vec<(JsValue, Vec<JsValue>)> {
        let now = Instant::now();
        let mut fired = Vec::new();
        let mut timers = self.timers.borrow_mut();
        let mut i = 0;

        while i < timers.len() {
            if timers[i].fire_at <= now {
                let entry = timers.remove(i);
                trace!(timer_id = entry.id, kind = ?entry.kind, "Timer fired");
                fired.push((entry.handler.clone(), Vec::new()));

                if entry.kind == TimerKind::Interval {
                    let new_fire = now + entry.interval;
                    timers.push(TimerEntry {
                        id: entry.id,
                        fire_at: new_fire,
                        interval: entry.interval,
                        kind: TimerKind::Interval,
                        handler: entry.handler,
                    });
                }
            } else {
                i += 1;
            }
        }

        fired
    }

    pub fn task_queue(&self) -> Rc<RefCell<TaskQueue>> {
        Rc::clone(&self.task_queue)
    }

    pub fn enqueue_microtask(&self, task: JsValue) {
        self.task_queue.borrow_mut().enqueue_microtask(task);
    }

    pub fn enqueue_macrotask(&self, handler: JsValue, args: Vec<JsValue>) {
        self.task_queue
            .borrow_mut()
            .enqueue_macrotask(handler, args);
    }

    pub fn process_microtasks(&self) -> Vec<JsValue> {
        self.task_queue.borrow_mut().drain_microtasks()
    }

    pub fn next_macrotask(&self) -> Option<Macrotask> {
        self.task_queue.borrow_mut().next_macrotask()
    }

    pub fn next_timer_id(&self) -> u64 {
        let id = self.next_timer_id.get();
        self.next_timer_id.set(id + 1);
        id
    }
}

impl Default for EventLoop {
    fn default() -> Self {
        Self::new()
    }
}
