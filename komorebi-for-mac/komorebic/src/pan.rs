//! Reading the stream of gestures that `komorebic pan --stream` is given, which is what a program
//! that reads a touchpad writes for a swipe of fingers, and telling komorebi what the fingers do.
//!
//! The format is the gesture stream that whkd writes on Windows, and anything that writes it can
//! be piped in: one message to a line, a name and then `key=value` fields in any order, the
//! distances in centimetres. A field that is not known is ignored, and so is a message that is not
//! known.

use komorebi_client::PanDirection;
use komorebi_client::PanGesture;
use komorebi_client::SocketMessage;
use std::collections::VecDeque;
use std::io;
use std::io::BufRead;
use std::sync::mpsc;

/// How far the fingers go with `--paging` for each step, in centimetres
pub const PAGE_CM: f64 = 2.0;

/// One thing that the fingers do
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Message {
    Begin,
    Move { dx: f64, dy: f64, t: f64 },
    End,
    Cancel,
}

/// What a line of the stream comes to
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Message(Message),
    /// Nothing to do: a line with nothing in it, or a comment, or a message that is not known
    Ignored,
    /// A message that is known, and is not what it should be
    Bad(String),
}

/// What a line of the stream says
#[must_use]
pub fn parse_line(line: &str) -> Line {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Line::Ignored;
    }

    let mut words = line.split_whitespace();
    let Some(name) = words.next() else {
        return Line::Ignored;
    };

    let fields: Vec<(&str, &str)> = words.filter_map(|word| word.split_once('=')).collect();
    let number = |key: &str| -> Option<Result<f64, String>> {
        fields
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| {
                value
                    .parse::<f64>()
                    .ok()
                    .filter(|number| number.is_finite())
                    .ok_or_else(|| format!("{key}={value} is not a number"))
            })
    };

    match name {
        "begin" => Line::Message(Message::Begin),
        "end" => Line::Message(Message::End),
        "cancel" => Line::Message(Message::Cancel),
        "move" => {
            let dx = match number("dx") {
                Some(Ok(dx)) => dx,
                Some(Err(error)) => return Line::Bad(error),
                None => return Line::Bad(String::from("a move has no dx")),
            };
            let t = match number("t") {
                Some(Ok(t)) => t,
                Some(Err(error)) => return Line::Bad(error),
                None => return Line::Bad(String::from("a move has no t")),
            };
            let dy = match number("dy") {
                Some(Ok(dy)) => dy,
                Some(Err(error)) => return Line::Bad(error),
                None => 0.0,
            };

            Line::Message(Message::Move { dx, dy, t })
        }
        _ => Line::Ignored,
    }
}

/// What `--stream` is asked to do with the distances
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// What the distances are multiplied by
    pub sensitivity: f64,
    /// The strip goes the other way to the fingers
    pub reversed: bool,
    /// One step for each [`PAGE_CM`] that the fingers go, and not the strip going with them
    pub paging: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            sensitivity: 1.0,
            reversed: false,
            paging: false,
        }
    }
}

/// Works out what komorebi is told for what the fingers do
#[derive(Debug)]
pub struct Forwarder {
    options: Options,
    /// Whether a gesture is going on
    active: bool,
    /// How far the fingers have gone since the last step, with paging
    page: f64,
}

impl Forwarder {
    #[must_use]
    pub const fn new(options: Options) -> Self {
        Self {
            options,
            active: false,
            page: 0.0,
        }
    }

    /// What komorebi is told for this. Fingers that move when there was no beginning, and an end
    /// or a cancel for a gesture that is not going on, are not anything.
    pub fn handle(&mut self, message: Message) -> Vec<SocketMessage> {
        let gesture = |gesture| vec![SocketMessage::PanGesture(gesture)];

        match message {
            Message::Begin => {
                self.active = true;
                self.page = 0.0;

                if self.options.paging {
                    Vec::new()
                } else {
                    gesture(PanGesture::Begin)
                }
            }
            Message::Move { dx, dy, t } if self.active => {
                let sign = if self.options.reversed { -1.0 } else { 1.0 };
                let dx = dx * self.options.sensitivity * sign;
                let dy = dy * self.options.sensitivity;

                if self.options.paging {
                    self.page += dx;
                    self.pages()
                } else {
                    gesture(PanGesture::Move { dx, dy, t })
                }
            }
            Message::End if self.active => {
                self.active = false;

                if self.options.paging {
                    Vec::new()
                } else {
                    gesture(PanGesture::End)
                }
            }
            Message::Cancel if self.active => {
                self.active = false;

                if self.options.paging {
                    Vec::new()
                } else {
                    gesture(PanGesture::Cancel)
                }
            }
            _ => Vec::new(),
        }
    }

    /// The stream is over: a gesture that is still going on was not finished, so it is given up
    pub fn finish(&mut self) -> Vec<SocketMessage> {
        let unfinished = self.active && !self.options.paging;
        self.active = false;

        if unfinished {
            vec![SocketMessage::PanGesture(PanGesture::Cancel)]
        } else {
            Vec::new()
        }
    }

    /// The steps that what the fingers have gone since the last one comes to. Fingers that go to
    /// the right take the strip with them, which is to look to the left.
    fn pages(&mut self) -> Vec<SocketMessage> {
        let mut steps = Vec::new();

        while self.page.abs() >= PAGE_CM {
            let direction = if self.page > 0.0 {
                PanDirection::Left
            } else {
                PanDirection::Right
            };

            self.page -= PAGE_CM.copysign(self.page);
            steps.push(SocketMessage::Pan(direction));
        }

        steps
    }
}

/// The next message to handle, with the moves that follow it added to it if it is one: when
/// the stream comes faster than it can be handled, the distances add up and the time is the
/// last one, as whkd says that they may.
pub fn next_merged(queue: &mut VecDeque<Message>) -> Option<Message> {
    let first = queue.pop_front()?;
    let Message::Move {
        mut dx,
        mut dy,
        mut t,
    } = first
    else {
        return Some(first);
    };

    while let Some(Message::Move {
        dx: more_dx,
        dy: more_dy,
        t: later,
    }) = queue.front().copied()
    {
        dx += more_dx;
        dy += more_dy;
        t = later;
        queue.pop_front();
    }

    Some(Message::Move { dx, dy, t })
}

/// Where komorebi is told what the fingers do
pub trait Sink {
    fn send(&mut self, message: &SocketMessage) -> io::Result<()>;

    /// The gesture that was being told is over
    fn gesture_over(&mut self);
}

/// A way of telling komorebi things, in order
pub trait Link {
    fn send(&mut self, message: &SocketMessage) -> io::Result<()>;
}

impl Link for komorebi_client::Connection {
    fn send(&mut self, message: &SocketMessage) -> io::Result<()> {
        komorebi_client::Connection::send(self, message)
    }
}

/// Tells komorebi what the fingers do over one connection for each gesture. Messages that are
/// sent on a connection each are not handled in the order that they were sent, so a gesture is
/// told on one, which is made when it is first needed and let go of when the gesture is over.
/// If it cannot be written to the connection is made again once, as komorebi may have let go of
/// one that nothing has been written to for a while.
pub struct Socket<L, F> {
    open: F,
    link: Option<L>,
}

impl<L: Link, F: FnMut() -> io::Result<L>> Socket<L, F> {
    pub const fn new(open: F) -> Self {
        Self { open, link: None }
    }

    fn try_send(&mut self, message: &SocketMessage) -> io::Result<()> {
        if self.link.is_none() {
            self.link = Some((self.open)()?);
        }

        match self.link.as_mut() {
            Some(link) => link.send(message),
            None => Ok(()),
        }
    }
}

impl<L: Link, F: FnMut() -> io::Result<L>> Sink for Socket<L, F> {
    fn send(&mut self, message: &SocketMessage) -> io::Result<()> {
        if self.try_send(message).is_err() {
            self.link = None;
            return self.try_send(message);
        }

        Ok(())
    }

    fn gesture_over(&mut self) {
        self.link = None;
    }
}

/// Read gestures from `input` until it closes and tell komorebi what the fingers do with `sink`
pub fn run(
    input: impl BufRead + Send + 'static,
    options: Options,
    sink: &mut impl Sink,
) -> io::Result<()> {
    let (sender, receiver) = mpsc::channel();

    std::thread::spawn(move || {
        for line in input.lines() {
            let Ok(line) = line else {
                break;
            };

            match parse_line(&line) {
                Line::Message(message) => {
                    if sender.send(message).is_err() {
                        break;
                    }
                }
                Line::Ignored => {}
                Line::Bad(why) => eprintln!("pan: {why}: {line}"),
            }
        }
    });

    let mut forwarder = Forwarder::new(options);
    let mut queue = VecDeque::new();

    while let Ok(first) = receiver.recv() {
        queue.push_back(first);
        queue.extend(receiver.try_iter());

        while let Some(message) = next_merged(&mut queue) {
            let over = matches!(message, Message::End | Message::Cancel);

            for told in forwarder.handle(message) {
                sink.send(&told)?;
            }

            if over {
                sink.gesture_over();
            }
        }
    }

    for told in forwarder.finish() {
        sink.send(&told)?;
    }
    sink.gesture_over();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRANSCRIPT: &str = include_str!("../fixtures/gesture-stream.transcript.txt");

    fn gestures(told: &[SocketMessage]) -> Vec<PanGesture> {
        told.iter()
            .filter_map(|message| match message {
                SocketMessage::PanGesture(gesture) => Some(*gesture),
                _ => None,
            })
            .collect()
    }

    fn messages(text: &str) -> Vec<Message> {
        text.lines()
            .filter_map(|line| match parse_line(line) {
                Line::Message(message) => Some(message),
                _ => None,
            })
            .collect()
    }

    /// What is told, and how many gestures have been over
    #[derive(Default)]
    struct Recorder {
        told: Vec<SocketMessage>,
        over: usize,
    }

    impl Sink for Recorder {
        fn send(&mut self, message: &SocketMessage) -> io::Result<()> {
            self.told.push(message.clone());
            Ok(())
        }

        fn gesture_over(&mut self) {
            self.over += 1;
        }
    }

    /// A link that remembers what it was written, and whose connections are numbered
    struct Wire {
        connection: usize,
        written: Written,
        fail_next: Failing,
    }

    impl Link for Wire {
        fn send(&mut self, message: &SocketMessage) -> io::Result<()> {
            if self.fail_next.replace(false) {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "let go of"));
            }

            self.written
                .borrow_mut()
                .push((self.connection, message.clone()));
            Ok(())
        }
    }

    type Written = std::rc::Rc<std::cell::RefCell<Vec<(usize, SocketMessage)>>>;

    type Failing = std::rc::Rc<std::cell::Cell<bool>>;

    #[allow(clippy::type_complexity)]
    fn wire() -> (
        Socket<Wire, impl FnMut() -> io::Result<Wire>>,
        Written,
        Failing,
    ) {
        let written = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let fail_next = std::rc::Rc::new(std::cell::Cell::new(false));
        let mut made = 0;

        let socket = Socket::new({
            let written = written.clone();
            let fail_next = fail_next.clone();
            move || {
                made += 1;
                Ok(Wire {
                    connection: made,
                    written: written.clone(),
                    fail_next: fail_next.clone(),
                })
            }
        });

        (socket, written, fail_next)
    }

    #[test]
    fn a_gesture_is_told_on_one_connection_and_the_next_has_its_own() {
        let (mut socket, written, _) = wire();
        let message = |gesture| SocketMessage::PanGesture(gesture);
        let moved = PanGesture::Move {
            dx: 1.0,
            dy: 0.0,
            t: 1.0,
        };

        socket.send(&message(PanGesture::Begin)).unwrap();
        socket.send(&message(moved)).unwrap();
        socket.send(&message(moved)).unwrap();
        socket.send(&message(PanGesture::End)).unwrap();
        socket.gesture_over();
        socket.send(&message(PanGesture::Begin)).unwrap();

        let connections: Vec<_> = written.borrow().iter().map(|(number, _)| *number).collect();
        assert_eq!(connections, vec![1, 1, 1, 1, 2]);
    }

    #[test]
    fn a_connection_that_komorebi_let_go_of_is_made_again_once() {
        let (mut socket, written, fail_next) = wire();
        let message = SocketMessage::PanGesture(PanGesture::Begin);

        socket.send(&message).unwrap();
        fail_next.set(true);
        socket.send(&message).unwrap();

        let connections: Vec<_> = written.borrow().iter().map(|(number, _)| *number).collect();
        assert_eq!(
            connections,
            vec![1, 2],
            "the second was written on a new one"
        );
    }

    #[test]
    fn a_message_that_cannot_be_written_twice_is_an_error() {
        let mut socket = Socket::new(|| -> io::Result<Wire> {
            Err(io::Error::new(io::ErrorKind::NotFound, "no komorebi"))
        });

        assert!(
            socket
                .send(&SocketMessage::PanGesture(PanGesture::Begin))
                .is_err()
        );
    }

    #[test]
    fn what_is_told_ends_each_gesture_that_ends_and_the_stream_that_stops() {
        let input = b"begin\nmove dx=1 t=1\nend\nbegin\nmove dx=1 t=2\n".to_vec();
        let mut sink = Recorder::default();

        run(io::Cursor::new(input), Options::default(), &mut sink).unwrap();

        // the first ended, and the second was given up when the stream stopped
        assert_eq!(sink.over, 2);
        assert_eq!(gestures(&sink.told).last(), Some(&PanGesture::Cancel));
    }

    fn step(dx: f64) -> Message {
        Message::Move {
            dx,
            dy: 0.0,
            t: 0.0,
        }
    }

    #[test]
    fn what_whkd_writes_is_read_as_it_was_written() {
        assert_eq!(
            messages(TRANSCRIPT),
            vec![
                Message::Begin,
                Message::Move {
                    dx: 0.42,
                    dy: -0.03,
                    t: 118_345.0
                },
                Message::Move {
                    dx: 0.51,
                    dy: -0.02,
                    t: 118_353.0
                },
                Message::Move {
                    dx: 0.47,
                    dy: 0.0,
                    t: 118_361.0
                },
                Message::End,
            ]
        );
    }

    /// What whkd writes for a swipe of three fingers that goes on for about two seconds, a
    /// flick, and one that eases to a stop: `whkd touchpad --demo <name> --instant`
    const LONG: &str = include_str!("../fixtures/pan3-long.transcript.txt");
    const FLICK: &str = include_str!("../fixtures/pan3-flick.transcript.txt");
    const SLOW: &str = include_str!("../fixtures/pan3-slow.transcript.txt");

    /// How many lines of a transcript are not part of the stream: whkd's demo says which swipe
    /// it saw, as `swipe swipe3right`, among them, and nothing says that the program that it
    /// runs is told it, so it is ignored like any other message that is not known
    fn others(transcript: &str) -> usize {
        transcript
            .lines()
            .filter(|line| line.starts_with("swipe "))
            .count()
    }

    #[test]
    fn every_line_that_whkd_writes_for_a_swipe_is_read_and_nothing_is_said_to_be_wrong() {
        for (transcript, lines) in [(LONG, 218), (FLICK, 18), (SLOW, 67)] {
            assert_eq!(transcript.lines().count(), lines);

            for line in transcript.lines() {
                match parse_line(line) {
                    Line::Message(_) => {}
                    Line::Ignored => assert!(line.starts_with("swipe "), "ignored: {line}"),
                    Line::Bad(why) => panic!("{why}: {line}"),
                }
            }

            let messages = messages(transcript);
            assert_eq!(messages.first(), Some(&Message::Begin));
            assert_eq!(messages.last(), Some(&Message::End));
            assert_eq!(messages.len(), lines - others(transcript));
        }

        assert_eq!(others(LONG) + others(FLICK) + others(SLOW), 2);
    }

    #[test]
    fn a_swipe_is_told_to_komorebi_move_for_move() {
        for transcript in [LONG, FLICK, SLOW] {
            let mut forwarder = Forwarder::new(Options::default());
            let told: Vec<_> = messages(transcript)
                .into_iter()
                .flat_map(|message| forwarder.handle(message))
                .collect();

            let gestures = gestures(&told);
            assert_eq!(
                gestures.len(),
                transcript.lines().count() - others(transcript)
            );
            assert_eq!(gestures.first(), Some(&PanGesture::Begin));
            assert_eq!(gestures.last(), Some(&PanGesture::End));

            // the distances add up to how far the fingers went across the touchpad
            let across: f64 = gestures
                .iter()
                .filter_map(|gesture| match gesture {
                    PanGesture::Move { dx, .. } => Some(*dx),
                    _ => None,
                })
                .sum();
            assert!(across > 1.0, "{across}");
        }
    }

    #[test]
    fn a_field_that_is_not_known_is_not_anything_and_nor_is_a_message() {
        // x, y, w, h, and a field of the future, which whkd says to ignore without a word
        assert_eq!(
            parse_line("move dx=1.5 dy=0.25 x=4.420 y=1.970 t=10 pressure=0.7"),
            Line::Message(Message::Move {
                dx: 1.5,
                dy: 0.25,
                t: 10.0
            })
        );
        assert_eq!(
            parse_line("begin fingers=3 x=4.000 y=2.000 w=10.500 h=7.200 t=118337"),
            Line::Message(Message::Begin)
        );
        assert_eq!(
            parse_line("end x=5.4 y=1.95 t=118369"),
            Line::Message(Message::End)
        );
        assert_eq!(parse_line("pinch scale=1.1"), Line::Ignored);
        assert_eq!(parse_line(""), Line::Ignored);
        assert_eq!(parse_line("   "), Line::Ignored);
        assert_eq!(parse_line("# a comment"), Line::Ignored);
    }

    #[test]
    fn a_move_that_is_not_what_it_should_be_is_said_to_be() {
        assert!(matches!(parse_line("move dy=1 t=3"), Line::Bad(_)), "no dx");
        assert!(matches!(parse_line("move dx=1 dy=1"), Line::Bad(_)), "no t");
        assert!(matches!(parse_line("move dx=wide t=3"), Line::Bad(_)));
        assert!(matches!(parse_line("move dx=NaN t=3"), Line::Bad(_)));
        assert_eq!(
            parse_line("move dx=2 t=3"),
            Line::Message(Message::Move {
                dx: 2.0,
                dy: 0.0,
                t: 3.0
            }),
            "dy is not needed"
        );
    }

    #[test]
    fn the_strip_goes_with_the_fingers_and_the_times_are_told_as_they_are() {
        let mut forwarder = Forwarder::new(Options::default());
        let told: Vec<_> = messages(TRANSCRIPT)
            .into_iter()
            .flat_map(|message| forwarder.handle(message))
            .collect();

        let gestures = gestures(&told);
        assert_eq!(gestures.len(), 5);
        assert_eq!(gestures[0], PanGesture::Begin);
        assert_eq!(
            gestures[1],
            PanGesture::Move {
                dx: 0.42,
                dy: -0.03,
                t: 118_345.0
            }
        );
        assert_eq!(gestures[4], PanGesture::End);
        assert!(
            forwarder.finish().is_empty(),
            "it ended, so nothing is left"
        );
    }

    #[test]
    fn sensitivity_scales_the_distances_and_reversing_flips_the_way_it_goes() {
        let mut forwarder = Forwarder::new(Options {
            sensitivity: 2.0,
            reversed: true,
            paging: false,
        });
        forwarder.handle(Message::Begin);

        let told = forwarder.handle(Message::Move {
            dx: 1.0,
            dy: 0.5,
            t: 1.0,
        });

        assert_eq!(
            gestures(&told),
            vec![PanGesture::Move {
                dx: -2.0,
                dy: 1.0,
                t: 1.0
            }]
        );
    }

    #[test]
    fn what_has_no_beginning_is_not_anything() {
        let mut forwarder = Forwarder::new(Options::default());

        assert!(forwarder.handle(step(1.0)).is_empty());
        assert!(forwarder.handle(Message::End).is_empty());
        assert!(forwarder.handle(Message::Cancel).is_empty());
        assert!(forwarder.finish().is_empty());
    }

    #[test]
    fn a_stream_that_stops_in_the_middle_of_a_gesture_gives_it_up() {
        let mut forwarder = Forwarder::new(Options::default());
        forwarder.handle(Message::Begin);
        forwarder.handle(step(1.0));

        assert_eq!(gestures(&forwarder.finish()), vec![PanGesture::Cancel]);
        assert!(forwarder.finish().is_empty(), "once");
    }

    #[test]
    fn fingers_that_go_far_enough_take_a_step_each_with_paging() {
        let mut forwarder = Forwarder::new(Options {
            paging: true,
            ..Options::default()
        });

        assert!(forwarder.handle(Message::Begin).is_empty());
        assert!(forwarder.handle(step(1.5)).is_empty(), "not far enough");

        // 1.5 and 0.7 is 2.2: a step to the left, as the strip goes right with the fingers
        let told = forwarder.handle(step(0.7));
        assert!(matches!(
            told.as_slice(),
            [SocketMessage::Pan(PanDirection::Left)]
        ));

        // 0.2 is left over, and with 4.0 more it is two steps
        let told = forwarder.handle(step(4.0));
        assert_eq!(told.len(), 2);

        // and the other way: 0.2 and 8.5 back is 8.3, which is four
        let told = forwarder.handle(step(-8.5));
        assert_eq!(told.len(), 4);
        assert!(
            told.iter()
                .all(|message| matches!(message, SocketMessage::Pan(PanDirection::Right)))
        );

        assert!(forwarder.handle(Message::End).is_empty());
        assert!(forwarder.finish().is_empty());
    }

    #[test]
    fn moves_that_wait_are_added_up_and_nothing_else_is_put_out_of_its_place() {
        let mut queue: VecDeque<Message> = [
            Message::Begin,
            Message::Move {
                dx: 1.0,
                dy: 0.5,
                t: 1.0,
            },
            Message::Move {
                dx: 2.0,
                dy: 0.25,
                t: 2.0,
            },
            Message::Move {
                dx: 4.0,
                dy: 0.0,
                t: 3.0,
            },
            Message::End,
        ]
        .into();

        assert_eq!(next_merged(&mut queue), Some(Message::Begin));
        assert_eq!(
            next_merged(&mut queue),
            Some(Message::Move {
                dx: 7.0,
                dy: 0.75,
                t: 3.0
            })
        );
        assert_eq!(next_merged(&mut queue), Some(Message::End));
        assert_eq!(next_merged(&mut queue), None);
    }

    #[test]
    fn a_stream_is_told_to_komorebi_in_order_and_given_up_if_it_stops() {
        let input = b"begin fingers=3\nmove dx=1.0 dy=0 t=1\nmove dx=2.0 dy=0 t=2\n".to_vec();
        let mut sink = Recorder::default();

        run(io::Cursor::new(input), Options::default(), &mut sink).unwrap();

        let gestures = gestures(&sink.told);
        assert_eq!(gestures[0], PanGesture::Begin);
        assert!(matches!(gestures[1], PanGesture::Move { .. }));
        assert_eq!(gestures.last(), Some(&PanGesture::Cancel));
    }
}
