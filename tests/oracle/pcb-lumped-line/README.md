# Microstrip between lumped ports

The microstrip of `pcb-microstrip-fem`, 5 mm long, fed at both ends by `pcb.port`: a 50 Ω sheet
from the ground floor up to the trace's end, the trace cut square there. With lumped ports alone
`solve.frequency` gives S of their power waves (Kurokawa). The line's |S11| and |S21| between
50 Ω are compared with Hammerstad and Jensen's line in its ABCD matrix; the phase carries the
feeds' own inductance besides the line's.
