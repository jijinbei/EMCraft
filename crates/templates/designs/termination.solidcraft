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
  "name": "Termination",
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
      "expr": "105",
      "unit": "mm",
      "model": false
    },
    {
      "name": "w",
      "expr": "90",
      "unit": "mm",
      "model": false
    }
  ],
  "features": [
    {
      "id": 1,
      "name": "Box1",
      "suppressed": false,
      "body_names": [
        "guide"
      ],
      "param_names": [
        "d1",
        "d2",
        "d3"
      ],
      "type": "box",
      "corner": [
        0.0,
        0.0,
        0.0
      ],
      "length": "a",
      "width": "l",
      "height": "b",
      "operation": "new_body"
    },
    {
      "id": 2,
      "name": "wedge",
      "suppressed": false,
      "param_names": [
        "d4"
      ],
      "type": "sketch",
      "plane": {
        "type": "offset",
        "base": {
          "type": "origin",
          "name": "XY"
        },
        "distance": "1"
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
              1.0,
              104.0
            ],
            "fixed": true,
            "exprs": {
              "y": "l - 1"
            }
          },
          {
            "id": "p2",
            "pos": [
              11.43,
              15.0
            ],
            "fixed": true,
            "exprs": {
              "x": "a / 2",
              "y": "l - w"
            }
          },
          {
            "id": "p3",
            "pos": [
              21.86,
              104.0
            ],
            "fixed": true,
            "exprs": {
              "x": "a - 1",
              "y": "l - 1"
            }
          }
        ],
        "curves": [
          {
            "id": "l1",
            "type": "line",
            "a": 1,
            "b": 2,
            "construction": false
          },
          {
            "id": "l2",
            "type": "line",
            "a": 2,
            "b": 3,
            "construction": false
          },
          {
            "id": "l3",
            "type": "line",
            "a": 3,
            "b": 1,
            "construction": false
          }
        ],
        "constraints": [],
        "counters": {
          "l": 3,
          "p": 3
        }
      }
    },
    {
      "id": 3,
      "name": "Extrude1",
      "suppressed": false,
      "body_names": [
        "absorber"
      ],
      "param_names": [
        "d5"
      ],
      "type": "extrude",
      "sketch": 2,
      "profiles": {
        "type": "all"
      },
      "extent": {
        "distance": "b - 2",
        "convention": "total_distance",
        "direction": "positive"
      },
      "operation": "new_body"
    }
  ],
  "next_id": 4,
  "next_component_color_index": 0
}