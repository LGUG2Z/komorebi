```
Move the view of the Scrolling layout a column to the left or the right, or with the fingers on a touchpad, which does nothing in any other layout

Usage: pan [OPTIONS] [DIRECTION]

Arguments:
  [DIRECTION]
          The way that the view goes
          
          [possible values: left, right]

Options:
      --stream
          Read the gestures of fingers on a touchpad from standard input, as whkd writes them for a swipe of three or four fingers, and pan with them

      --sensitivity <SENSITIVITY>
          What the distances of a gesture are multiplied by, which is 1 if it is left out, with --stream

      --reversed
          Go the other way to the fingers, with --stream

      --paging
          Take a step for every two centimetres that the fingers go, and not move the view with them, with --stream

  -h, --help
          Print help

```
