{
  "format": "solidcraft/51",
  "format_pending": [
    "bridged-counterbore",
    "embedded-surface-textures",
    "fillet-turn",
    "hole-direction-own-faces",
    "intersection-curve-sketch-curves",
    "point-cloud-segments-and-triangles",
    "revolve-axis-origin-expressions",
    "transferred-profile-frame",
    "two-sided-extrude-fixed-sides"
  ],
  "name": "H-plane bend with a radius",
  "units": "mm",
  "params": [
    {
      "name": "a",
      "expr": "22.86",
      "unit": "mm",
      "model": false
    },
    {
      "name": "b",
      "expr": "10.16",
      "unit": "mm",
      "model": false
    },
    {
      "name": "l",
      "expr": "20",
      "unit": "mm",
      "model": false
    },
    {
      "name": "r",
      "expr": "28.6",
      "unit": "mm",
      "model": false
    }
  ],
  "features": [
    {
      "id": 1,
      "name": "outline",
      "suppressed": false,
      "type": "sketch",
      "plane": {
        "type": "origin",
        "name": "XY"
      },
      "sketch": {
        "points": [
          {
            "id": "origin",
            "pos": [
              0.0,
              0.0
            ],
            "fixed": true
          },
          {
            "id": "p1",
            "pos": [
              0.0,
              20.0
            ],
            "fixed": true,
            "exprs": {
              "y": "l"
            }
          },
          {
            "id": "p2",
            "pos": [
              22.86,
              0.0
            ],
            "fixed": true,
            "exprs": {
              "x": "a"
            }
          },
          {
            "id": "p3",
            "pos": [
              22.86,
              20.0
            ],
            "fixed": true,
            "exprs": {
              "x": "a",
              "y": "l"
            }
          },
          {
            "id": "l1.end",
            "pos": [
              0.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "p4",
            "pos": [
              51.46,
              20.0
            ],
            "fixed": true,
            "exprs": {
              "x": "a + r",
              "y": "l"
            }
          },
          {
            "id": "p5",
            "pos": [
              22.86,
              20.0
            ],
            "fixed": true,
            "exprs": {
              "x": "a",
              "y": "l"
            }
          },
          {
            "id": "p6",
            "pos": [
              51.46,
              48.6
            ],
            "fixed": true,
            "exprs": {
              "x": "a + r",
              "y": "l + r"
            }
          },
          {
            "id": "p7",
            "pos": [
              51.46,
              48.6
            ],
            "fixed": true,
            "exprs": {
              "x": "a + r",
              "y": "l + r"
            }
          },
          {
            "id": "p8",
            "pos": [
              71.46000000000001,
              48.6
            ],
            "fixed": true,
            "exprs": {
              "x": "a + r + l",
              "y": "l + r"
            }
          },
          {
            "id": "p9",
            "pos": [
              71.46000000000001,
              71.46000000000001
            ],
            "fixed": true,
            "exprs": {
              "x": "a + r + l",
              "y": "l + r + a"
            }
          },
          {
            "id": "p10",
            "pos": [
              51.46,
              71.46000000000001
            ],
            "fixed": true,
            "exprs": {
              "x": "a + r",
              "y": "l + r + a"
            }
          },
          {
            "id": "p11",
            "pos": [
              51.46,
              20.0
            ],
            "fixed": true,
            "exprs": {
              "x": "a + r",
              "y": "l"
            }
          },
          {
            "id": "p12",
            "pos": [
              51.46,
              71.46000000000001
            ],
            "fixed": true,
            "exprs": {
              "x": "a + r",
              "y": "l + r + a"
            }
          },
          {
            "id": "p13",
            "pos": [
              0.0,
              20.0
            ],
            "fixed": true,
            "exprs": {
              "y": "l"
            }
          }
        ],
        "curves": [
          {
            "id": "l1",
            "type": "line",
            "a": 1,
            "b": 4,
            "construction": false
          },
          {
            "id": "l2",
            "type": "line",
            "a": 4,
            "b": 2,
            "construction": false
          },
          {
            "id": "l3",
            "type": "line",
            "a": 2,
            "b": 3,
            "construction": false
          },
          {
            "id": "a1",
            "type": "arc",
            "c": 5,
            "a": 7,
            "b": 6,
            "construction": false,
            "reversed": true
          },
          {
            "id": "l4",
            "type": "line",
            "a": 8,
            "b": 9,
            "construction": false
          },
          {
            "id": "l5",
            "type": "line",
            "a": 9,
            "b": 10,
            "construction": false
          },
          {
            "id": "l6",
            "type": "line",
            "a": 10,
            "b": 11,
            "construction": false
          },
          {
            "id": "a2",
            "type": "arc",
            "c": 12,
            "a": 13,
            "b": 14,
            "construction": false
          }
        ],
        "constraints": [
          {
            "id": "k1",
            "type": "coincident",
            "p": 0,
            "q": 4
          }
        ],
        "counters": {
          "a": 2,
          "k": 1,
          "l": 6,
          "p": 13
        }
      }
    },
    {
      "id": 2,
      "name": "Extrude1",
      "suppressed": false,
      "body_names": [
        "bend"
      ],
      "param_names": [
        "d1"
      ],
      "type": "extrude",
      "sketch": 1,
      "profiles": {
        "type": "all"
      },
      "extent": {
        "distance": "b",
        "convention": "total_distance",
        "direction": "positive"
      },
      "operation": "new_body"
    }
  ],
  "next_id": 3,
  "next_component_color_index": 0
}