macro_rules! keys {
    ($($name:ident),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Key {
            $($name),*
        }

        impl Key {
            pub const ALL: &[Key] = &[$(Key::$name),*];
        }

        impl From<Key> for minifb::Key {
            fn from(key: Key) -> Self {
                match key {
                    $(Key::$name => minifb::Key::$name),*
                }
            }
        }
    };
}

keys! {
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    Key0, Key1, Key2, Key3, Key4, Key5, Key6, Key7, Key8, Key9,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    Up, Down, Left, Right,
    Space, Enter, Escape, Tab, Backspace,
    LeftShift, RightShift, LeftCtrl, RightCtrl, LeftAlt, RightAlt,
}

const _: () = assert!(Key::ALL.len() <= u128::BITS as usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

impl MouseButton {
    pub const ALL: [MouseButton; 3] = [MouseButton::Left, MouseButton::Middle, MouseButton::Right];
}

impl From<MouseButton> for minifb::MouseButton {
    fn from(button: MouseButton) -> Self {
        match button {
            MouseButton::Left => minifb::MouseButton::Left,
            MouseButton::Middle => minifb::MouseButton::Middle,
            MouseButton::Right => minifb::MouseButton::Right,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Controls {
    keys: u128,
    prev_keys: u128,
    buttons: u8,
    prev_buttons: u8,
    mouse: Option<[f32; 2]>,
}

impl Controls {
    pub fn down(&self, key: Key) -> bool {
        self.keys & key_bit(key) != 0
    }

    pub fn pressed(&self, key: Key) -> bool {
        self.keys & !self.prev_keys & key_bit(key) != 0
    }

    pub fn released(&self, key: Key) -> bool {
        !self.keys & self.prev_keys & key_bit(key) != 0
    }

    pub fn axis(&self, negative: &[Key], positive: &[Key]) -> f32 {
        let held = |keys: &[Key]| keys.iter().any(|&key| self.down(key));
        f32::from(u8::from(held(positive))) - f32::from(u8::from(held(negative)))
    }

    pub fn mouse(&self) -> Option<[f32; 2]> {
        self.mouse
    }

    pub fn button_down(&self, button: MouseButton) -> bool {
        self.buttons & button_bit(button) != 0
    }

    pub fn button_pressed(&self, button: MouseButton) -> bool {
        self.buttons & !self.prev_buttons & button_bit(button) != 0
    }

    pub(super) fn poll(
        &mut self,
        key_down: impl Fn(Key) -> bool,
        button_down: impl Fn(MouseButton) -> bool,
        mouse: Option<[f32; 2]>,
    ) {
        self.keys = Key::ALL
            .iter()
            .filter(|&&key| key_down(key))
            .fold(0, |bits, &key| bits | key_bit(key));
        self.buttons = MouseButton::ALL
            .iter()
            .filter(|&&button| button_down(button))
            .fold(0, |bits, &button| bits | button_bit(button));
        self.mouse = mouse;
    }

    pub(super) fn settle(&mut self) {
        self.prev_keys = self.keys;
        self.prev_buttons = self.buttons;
    }
}

fn key_bit(key: Key) -> u128 {
    1 << key as u32
}

fn button_bit(button: MouseButton) -> u8 {
    1 << button as u8
}

#[cfg(test)]
mod tests {
    use super::{Controls, Key, MouseButton};

    fn poll(controls: &mut Controls, keys: &[Key], buttons: &[MouseButton]) {
        controls.poll(|k| keys.contains(&k), |b| buttons.contains(&b), None);
    }

    #[test]
    fn pressed_fires_on_the_first_update_only() {
        let mut controls = Controls::default();
        poll(&mut controls, &[Key::Space], &[MouseButton::Left]);
        assert!(controls.pressed(Key::Space));
        assert!(controls.button_pressed(MouseButton::Left));
        controls.settle();

        poll(&mut controls, &[Key::Space], &[MouseButton::Left]);
        assert!(controls.down(Key::Space));
        assert!(!controls.pressed(Key::Space));
        assert!(!controls.button_pressed(MouseButton::Left));
        controls.settle();

        poll(&mut controls, &[], &[]);
        assert!(controls.released(Key::Space));
        assert!(!controls.down(Key::Space));
    }

    #[test]
    fn unsettled_press_survives_frames_without_an_update() {
        let mut controls = Controls::default();
        poll(&mut controls, &[Key::Enter], &[]);
        poll(&mut controls, &[Key::Enter], &[]);
        assert!(controls.pressed(Key::Enter));
    }

    #[test]
    fn axis_cancels_opposing_keys() {
        let mut controls = Controls::default();
        poll(&mut controls, &[Key::W, Key::Down], &[]);
        assert_eq!(controls.axis(&[Key::W, Key::Up], &[Key::S, Key::Down]), 0.0);
        poll(&mut controls, &[Key::Up], &[]);
        assert_eq!(
            controls.axis(&[Key::W, Key::Up], &[Key::S, Key::Down]),
            -1.0
        );
    }
}
