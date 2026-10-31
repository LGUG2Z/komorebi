# pan

```
Move the view of the Scrolling layout a column to the left or the right, or with the fingers on a
touchpad

The strip of columns moves by the width of a new column and comes to rest with a column against an
edge of the screen. Focus stays on the column that has it if that is still fully in view, and
otherwise goes to the nearest column that is. Only the Scrolling layout has a strip to move, so this
does nothing in the other layouts.

With --stream the strip follows the fingers as they go, 10 centimetres of travel being the width of
the screen, until they lift, and then comes to rest in the column that the speed that they lifted at
takes it to. Standard input is the gesture stream that whkd writes on Windows, one message to a
line: `begin`, `move dx=<cm> t=<ms>`, `end` and `cancel`. A gesture that is not finished when
standard input closes is given up, and the strip goes back to where it was.

Usage: komorebic pan [OPTIONS] [DIRECTION]

Arguments:
  [DIRECTION]
          The way that the view goes
          
          [possible values: left, right]

Options:
      --stream
          Read the gestures of fingers on a touchpad from standard input, in the gesture stream that
          whkd writes for a swipe of three or four fingers, and pan with them

      --sensitivity <SENSITIVITY>
          What the distances of a gesture are multiplied by, which is 1 if it is left out, with
          --stream

      --reversed
          Go the other way to the fingers, with --stream

      --paging
          Take a step for every two centimetres that the fingers go, and not move the view with
          them, with --stream

  -h, --help
          Print help (see a summary with '-h')

```