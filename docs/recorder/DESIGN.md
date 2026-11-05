# Design

This document records why the recorder is built the way it is. It is organized as questions, because each answer is a
decision that could have gone another way. The facts in it were measured or read from the code, and where a reason is a
judgment it says so. It is written in the first person only where the decision was the owner's.

## Why record the screen, the log, the events and the OS trace together?

Most of the komorebi bugs I see show up when a laptop is docked or undocked, or wakes when the screen is opened. To
reproduce one, I had to watch the screen, hand-grep komorebi's log and export the system log from Console, and there was
no way to say: at 00:12.5 in the video, what was komorebi doing, and what was the OS doing? The recorder puts the four on
one clock so that the question has an answer, and so that the answer also works the other way, from a log line to the
moment in the video.

OS-level tracing is in scope because the interesting seam is between komorebi and the system, and between the system and
the application being tiled; komorebi's own log only shows one side of each.

## Why one clock, and why that one?

Every record is stamped on the host's monotonic clock (`mach_absolute_time` on macOS, `QueryPerformanceCounter` on
Windows) because the two sources that cannot be re-stamped are already on it: ScreenCaptureKit frame timestamps and the
unified log's `machTimestamp` on a Mac, and Windows.Graphics.Capture frame times on Windows. komorebi's log lines carry
UTC wall-clock time, so they are mapped onto the master clock with paired readings of both clocks (anchors) sampled while
recording, interpolated piecewise so that a step or a sleep between two anchors does not smear the mapping.

The alignment is as good as that allows: video to OS trace within about one frame, and video to komorebi's log within
about one frame plus the error of the wall-clock anchor, tens of microseconds.

## Why are fields called `*_mach_ns` on Windows?

They hold nanoseconds on the master clock, whichever clock that was. The spelling is historical and was kept: renaming
them would make a session written by one build unreadable by another. The manifest's `host.clock` says which clock a
session was on.

## Why does the recorder never change komorebi's log level or lifecycle?

That was decided at the start: the recorder only observes. It warns when komorebi is logging at a level that leaves out
most of what the window manager decided, and says how to restart it, but it never restarts komorebi or changes its
settings. (The reason for the decision is not written down anywhere the design could be checked against; this entry
needs the owner's words.)

## Why does the accept loop never block, and why is an event stamped when its handler finished?

komorebi connects to each subscriber synchronously and sends a notification while it holds its own locks, so a recorder
that was slow to read would stall komorebi. The accept thread stamps the arrival, reads, and hands the event to a channel;
a writer thread does everything else. For the same reason the per-event window probe (`--probe-windows`) runs on the
writer thread and never on the accept thread.

komorebi sends the notification after it has handled the event, so the stamp is when the handler finished. The start of
the handler is the `processing event` line in komorebi's log, which is why `trace` pairs the two.

## Why ScreenCaptureKit through `objc2` and not the `screencapturekit` crate?

The `screencapturekit` crate needs a Swift bridge at build time, which complicates the nix build. `objc2-screen-capture-kit`
and its siblings exist at the versions the rest of komorebi already uses, so there is no Swift and no Xcode dependency.

## Why fragmented mp4, and why re-submit the last frame?

A recorder that is killed, or a machine that loses power, must not lose the recording, and a plain mp4 is unreadable
until it is finalized. Fragmented mp4 plays up to the last fragment. Two measured facts decided the rest: ScreenCaptureKit
only emits frames when the screen changes, and fragments are cut on media time, so a static display never flushed a
fragment; the capture therefore re-submits the last frame to keep it moving. And the duration `ffprobe` reports for a
truncated fragmented file under-reads, so recovery asks AVFoundation instead.

## Why a new file for every segment?

Docking and undocking, sleep and lock change what can be recorded, and I want the recording to carry on through all of
them without losing what was already captured. A display that disappears has its video finalized on the spot, so the file
is complete and playable whatever happens next, and a display that returns starts a new segment in a new file. Every
display's segments are listed in the manifest with their time spans, so a gap is a fact the session states rather than a
hole.

## Why raw files plus an index, and not one file or a database?

The raw files are written as the data arrives and are never duplicated or rewritten; `timeline.jsonl` only references byte
ranges in them, so the streams stay separate and can be reconciled or re-indexed later. The index can be rebuilt from the
raw files at any time (`reindex`), including after a crash and after a hand edit.

## Why does reading a session never ask which host it is on?

A session recorded on Windows is investigated on a Mac, and the other way round. The host and the komorebi flavor are
recorded in the manifest, and everything that reads a session asks the manifest.

## Why can a bug only be marked in the viewer?

The Record tab has no button for marking a bug, and a session's tab has one. I found it confusing to have two places to do
the same thing, so a bug is marked in a session's tab only. `komorebi-recorder annotate` writes the same annotations for a
script or an agent that has no window.

## Why is there one window?

Recording and reviewing used to be two windows, and the recorder's buttons for opening a recording started the other one.
Now the recorder is the pinned first tab of the one window and each recording is a tab after it, so there is one thing to
keep out of the video and out of komorebi's layout. It starts small, always on top, with only the Record tab, which is what
the old recorder window was for, and grows when a recording is opened.

## Why is the window untitled?

komorebi does not manage windows with an empty title, so an untitled window is never tiled: opening the recorder
does not move the layout of the machine whose bug is being recorded.

## Why is ffmpeg not part of the program?

ffmpeg is used only to extract frames (`frames`) and to make test videos, and never for capture. It is in the dev shell and
not in the packaged binary.

## Why is a komorebi source from a different commit "skewed" by files and not by commit?

The recorder's own commits advance the repository's head without touching komorebi, so comparing commits calls almost
everything skewed. The files that differ between the commit a session recorded and the commit indexed are what say whether
a line of the log can still be traced to the statement that wrote it.

## Why is the hidden-window check skipped on Windows?

komorebi for Windows hides a window by cloaking it where it is, so a window's rectangle says nothing about whether it is
hidden. The check that flags a window parked off screen on the showing workspace holds only where komorebi hides windows
by moving them, so it is skipped on Windows, and `--probe-windows` reads the real state from Windows instead.

## Why do the Windows rules say they are unverified?

They were written from komorebi for Windows' source. Until a recording of it has been read, an analysis that uses them says
so; `Rules::verified` is the flag, and it is flipped once the first checks on a Windows machine confirm the rules.
