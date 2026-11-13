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
  "name": "E-plane bend with a radius",
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
      "expr": "35",
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
              10.16,
              0.0
            ],
            "fixed": true,
            "exprs": {
              "x": "b"
            }
          },
          {
            "id": "p3",
            "pos": [
              10.16,
              20.0
            ],
            "fixed": true,
            "exprs": {
              "x": "b",
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
              45.16,
              20.0
            ],
            "fixed": true,
            "exprs": {
              "x": "b + r",
              "y": "l"
            }
          },
          {
            "id": "p5",
            "pos": [
              10.16,
              20.0
            ],
            "fixed": true,
            "exprs": {
              "x": "b",
              "y": "l"
            }
          },
          {
            "id": "p6",
            "pos": [
              45.16,
              55.0
            ],
            "fixed": true,
            "exprs": {
              "x": "b + r",
              "y": "l + r"
            }
          },
          {
            "id": "p7",
            "pos": [
              45.16,
              55.0
            ],
            "fixed": true,
            "exprs": {
              "x": "b + r",
              "y": "l + r"
            }
          },
          {
            "id": "p8",
            "pos": [
              65.16,
              55.0
            ],
            "fixed": true,
            "exprs": {
              "x": "b + r + l",
              "y": "l + r"
            }
          },
          {
            "id": "p9",
            "pos": [
              65.16,
              65.16
            ],
            "fixed": true,
            "exprs": {
              "x": "b + r + l",
              "y": "l + r + b"
            }
          },
          {
            "id": "p10",
            "pos": [
              45.16,
              65.16
            ],
            "fixed": true,
            "exprs": {
              "x": "b + r",
              "y": "l + r + b"
            }
          },
          {
            "id": "p11",
            "pos": [
              45.16,
              20.0
            ],
            "fixed": true,
            "exprs": {
              "x": "b + r",
              "y": "l"
            }
          },
          {
            "id": "p12",
            "pos": [
              45.16,
              65.16
            ],
            "fixed": true,
            "exprs": {
              "x": "b + r",
              "y": "l + r + b"
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
        "distance": "a",
        "convention": "total_distance",
        "direction": "positive"
      },
      "operation": "new_body"
    }
  ],
  "next_id": 3,
  "next_component_color_index": 0
}