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
  "name": "Magic tee",
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
      "name": "rp",
      "expr": "1",
      "unit": "mm",
      "model": false
    },
    {
      "name": "hp",
      "expr": "11",
      "unit": "mm",
      "model": false
    },
    {
      "name": "hx",
      "expr": "8",
      "unit": "mm",
      "model": false
    },
    {
      "name": "ez",
      "expr": "5",
      "unit": "mm",
      "model": false
    },
    {
      "name": "ey",
      "expr": "5",
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
              62.86,
              0.0
            ],
            "fixed": true,
            "exprs": {
              "x": "2 * l + a"
            }
          },
          {
            "id": "p2",
            "pos": [
              62.86,
              22.86
            ],
            "fixed": true,
            "exprs": {
              "x": "2 * l + a",
              "y": "a"
            }
          },
          {
            "id": "p3",
            "pos": [
              42.86,
              22.86
            ],
            "fixed": true,
            "exprs": {
              "x": "l + a",
              "y": "a"
            }
          },
          {
            "id": "p4",
            "pos": [
              42.86,
              42.86
            ],
            "fixed": true,
            "exprs": {
              "x": "l + a",
              "y": "l + a"
            }
          },
          {
            "id": "p5",
            "pos": [
              20.0,
              42.86
            ],
            "fixed": true,
            "exprs": {
              "x": "l",
              "y": "l + a"
            }
          },
          {
            "id": "p6",
            "pos": [
              20.0,
              22.86
            ],
            "fixed": true,
            "exprs": {
              "x": "l",
              "y": "a"
            }
          },
          {
            "id": "p7",
            "pos": [
              0.0,
              22.86
            ],
            "fixed": true,
            "exprs": {
              "y": "a"
            }
          },
          {
            "id": "l1.start",
            "pos": [
              0.0,
              0.0
            ],
            "fixed": false
          }
        ],
        "curves": [
          {
            "id": "l1",
            "type": "line",
            "a": 8,
            "b": 1,
            "construction": false
          },
          {
            "id": "l2",
            "type": "line",
            "a": 1,
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
            "id": "l4",
            "type": "line",
            "a": 3,
            "b": 4,
            "construction": false
          },
          {
            "id": "l5",
            "type": "line",
            "a": 4,
            "b": 5,
            "construction": false
          },
          {
            "id": "l6",
            "type": "line",
            "a": 5,
            "b": 6,
            "construction": false
          },
          {
            "id": "l7",
            "type": "line",
            "a": 6,
            "b": 7,
            "construction": false
          },
          {
            "id": "l8",
            "type": "line",
            "a": 7,
            "b": 8,
            "construction": false
          }
        ],
        "constraints": [
          {
            "id": "k1",
            "type": "coincident",
            "p": 0,
            "q": 8
          }
        ],
        "counters": {
          "k": 1,
          "l": 8,
          "p": 7
        }
      }
    },
    {
      "id": 2,
      "name": "Extrude1",
      "suppressed": false,
      "body_names": [
        "tee"
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
        "distance": "b + l",
        "convention": "total_distance",
        "direction": "positive"
      },
      "operation": "new_body"
    },
    {
      "id": 3,
      "name": "cap",
      "suppressed": false,
      "param_names": [
        "d2"
      ],
      "type": "sketch",
      "plane": {
        "type": "offset",
        "base": {
          "type": "origin",
          "name": "XY"
        },
        "distance": "b"
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
              26.35,
              0.0
            ],
            "fixed": true,
            "exprs": {
              "x": "l + a / 2 - b / 2"
            }
          },
          {
            "id": "p2",
            "pos": [
              26.35,
              22.86
            ],
            "fixed": true,
            "exprs": {
              "x": "l + a / 2 - b / 2",
              "y": "a"
            }
          },
          {
            "id": "p3",
            "pos": [
              36.51,
              22.86
            ],
            "fixed": true,
            "exprs": {
              "x": "l + a / 2 + b / 2",
              "y": "a"
            }
          },
          {
            "id": "p4",
            "pos": [
              36.51,
              0.0
            ],
            "fixed": true,
            "exprs": {
              "x": "l + a / 2 + b / 2"
            }
          },
          {
            "id": "p5",
            "pos": [
              62.86,
              0.0
            ],
            "fixed": true,
            "exprs": {
              "x": "2 * l + a"
            }
          },
          {
            "id": "p6",
            "pos": [
              62.86,
              22.86
            ],
            "fixed": true,
            "exprs": {
              "x": "2 * l + a",
              "y": "a"
            }
          },
          {
            "id": "p7",
            "pos": [
              42.86,
              22.86
            ],
            "fixed": true,
            "exprs": {
              "x": "l + a",
              "y": "a"
            }
          },
          {
            "id": "p8",
            "pos": [
              42.86,
              42.86
            ],
            "fixed": true,
            "exprs": {
              "x": "l + a",
              "y": "l + a"
            }
          },
          {
            "id": "p9",
            "pos": [
              20.0,
              42.86
            ],
            "fixed": true,
            "exprs": {
              "x": "l",
              "y": "l + a"
            }
          },
          {
            "id": "p10",
            "pos": [
              20.0,
              22.86
            ],
            "fixed": true,
            "exprs": {
              "x": "l",
              "y": "a"
            }
          },
          {
            "id": "p11",
            "pos": [
              0.0,
              22.86
            ],
            "fixed": true,
            "exprs": {
              "y": "a"
            }
          },
          {
            "id": "l1.start",
            "pos": [
              0.0,
              0.0
            ],
            "fixed": false
          }
        ],
        "curves": [
          {
            "id": "l1",
            "type": "line",
            "a": 12,
            "b": 1,
            "construction": false
          },
          {
            "id": "l2",
            "type": "line",
            "a": 1,
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
            "id": "l4",
            "type": "line",
            "a": 3,
            "b": 4,
            "construction": false
          },
          {
            "id": "l5",
            "type": "line",
            "a": 4,
            "b": 5,
            "construction": false
          },
          {
            "id": "l6",
            "type": "line",
            "a": 5,
            "b": 6,
            "construction": false
          },
          {
            "id": "l7",
            "type": "line",
            "a": 6,
            "b": 7,
            "construction": false
          },
          {
            "id": "l8",
            "type": "line",
            "a": 7,
            "b": 8,
            "construction": false
          },
          {
            "id": "l9",
            "type": "line",
            "a": 8,
            "b": 9,
            "construction": false
          },
          {
            "id": "l10",
            "type": "line",
            "a": 9,
            "b": 10,
            "construction": false
          },
          {
            "id": "l11",
            "type": "line",
            "a": 10,
            "b": 11,
            "construction": false
          },
          {
            "id": "l12",
            "type": "line",
            "a": 11,
            "b": 12,
            "construction": false
          }
        ],
        "constraints": [
          {
            "id": "k1",
            "type": "coincident",
            "p": 0,
            "q": 12
          }
        ],
        "counters": {
          "k": 1,
          "l": 12,
          "p": 11
        }
      }
    },
    {
      "id": 4,
      "name": "Extrude2",
      "suppressed": false,
      "body_names": [
        "cap"
      ],
      "param_names": [
        "d3"
      ],
      "type": "extrude",
      "sketch": 3,
      "profiles": {
        "type": "all"
      },
      "extent": {
        "distance": "l",
        "convention": "total_distance",
        "direction": "positive"
      },
      "operation": "new_body"
    },
    {
      "id": 5,
      "name": "Cylinder1",
      "suppressed": false,
      "body_names": [
        "post1"
      ],
      "param_names": [
        "d4",
        "d5"
      ],
      "type": "cylinder",
      "base": [
        "l + a / 2 - hx",
        "a + hp",
        0.0
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "rp",
      "height": "b + 2",
      "operation": "new_body"
    },
    {
      "id": 6,
      "name": "Cylinder2",
      "suppressed": false,
      "body_names": [
        "post2"
      ],
      "param_names": [
        "d6",
        "d7"
      ],
      "type": "cylinder",
      "base": [
        "l + a / 2 + hx",
        "a + hp",
        0.0
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "rp",
      "height": "b + 2",
      "operation": "new_body"
    },
    {
      "id": 7,
      "name": "Cylinder3",
      "suppressed": false,
      "body_names": [
        "post3"
      ],
      "param_names": [
        "d8",
        "d9"
      ],
      "type": "cylinder",
      "base": [
        "l + a / 2 - b / 2 - 2",
        "ey",
        "b + ez"
      ],
      "axis": [
        1.0,
        0.0,
        0.0
      ],
      "radius": "rp",
      "height": "b + 4",
      "operation": "new_body"
    }
  ],
  "next_id": 8,
  "next_component_color_index": 0
}