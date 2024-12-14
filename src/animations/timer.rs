#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

use crate::windows_api::SendHWND;
use crate::windows_api::WindowsApi;
use crate::windows_api::WM_APP_TIMER;

#[derive(Debug, PartialEq, Clone)]
pub enum TimerState {
    Running = 0,
    Paused = 1,
    Stopped = 2,
}

#[derive(Debug)]
pub struct AnimationTimer {
    running: Arc<AtomicBool>,
    state: Arc<AtomicUsize>,
}

impl AnimationTimer {
    pub fn start(hwnd: HWND, interval_ms: u64) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let state = Arc::new(AtomicUsize::new(TimerState::Running as usize)); // 0 = Running, 1 = Paused, 2 = Stopped
        let running_clone = running.clone();
        let state_clone = state.clone();

        // Wrap HWND in a struct that implements Send and Sync to move it into the thread
        let window = SendHWND(hwnd);

        thread::spawn(move || {
            let window_sent = window;
            let interval = Duration::from_millis(interval_ms);
            let mut next_tick = Instant::now() + interval;

            while running_clone.load(Ordering::SeqCst) {
                let now = Instant::now();

                if state_clone.load(Ordering::SeqCst) == TimerState::Paused as usize {
                    thread::sleep(Duration::from_millis(interval_ms)); // Sleep to prevent busy-waiting
                    continue;
                }

                if now >= next_tick {
                    if let Err(e) = WindowsApi::post_message_w(
                        window_sent.0,
                        WM_APP_TIMER,
                        WPARAM(0),
                        LPARAM(0),
                    ) {
                        error!("could not send animation timer message: {e}");
                        break;
                    }
                    next_tick += interval; // Schedule the next tick
                }
                // Sleep for the remaining time until the next tick
                thread::sleep(next_tick.saturating_duration_since(Instant::now()));
            }
            // Timer stopped
            state_clone.store(TimerState::Stopped as usize, Ordering::SeqCst);
        });

        // Return the timer instance
        Self { running, state }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.state
            .store(TimerState::Stopped as usize, Ordering::SeqCst);
    }

    pub fn pause(&mut self) {
        self.state
            .store(TimerState::Paused as usize, Ordering::SeqCst);
    }

    pub fn resume(&mut self) {
        self.state
            .store(TimerState::Running as usize, Ordering::SeqCst);
    }

    pub fn get_state(&self) -> TimerState {
        let state_value = self.state.load(Ordering::SeqCst);
        match state_value {
            0 => TimerState::Running,
            1 => TimerState::Paused,
            2 => TimerState::Stopped,
            _ => panic!("Invalid state value!"),
        }
    }
}
