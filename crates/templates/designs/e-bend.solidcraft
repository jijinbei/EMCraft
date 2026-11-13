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
  "name": "E-plane bend",
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
      "name": "mitre",
      "expr": "8",
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
              10.16,
              0.0
            ],
            "fixed": true,
            "exprs": {
              "x": "b"
            }
          },
          {
            "id": "p2",
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
            "id": "p3",
            "pos": [
              30.16,
              20.0
            ],
            "fixed": true,
            "exprs": {
              "x": "l + b",
              "y": "l"
            }
          },
          {
            "id": "p4",
            "pos": [
              30.16,
              30.16
            ],
            "fixed": true,
            "exprs": {
              "x": "l + b",
              "y": "l + b"
            }
          },
          {
            "id": "p5",
            "pos": [
              8.0,
              30.16
            ],
            "fixed": true,
            "exprs": {
              "x": "mitre",
              "y": "l + b"
            }
          },
          {
            "id": "p6",
            "pos": [
              0.0,
              22.16
            ],
            "fixed": true,
            "exprs": {
              "y": "l + b - mitre"
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
            "a": 7,
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
          }
        ],
        "constraints": [
          {
            "id": "k1",
            "type": "coincident",
            "p": 0,
            "q": 7
          }
        ],
        "counters": {
          "k": 1,
          "l": 7,
          "p": 6
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