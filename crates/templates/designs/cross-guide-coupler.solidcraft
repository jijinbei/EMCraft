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
  "name": "Cross-guide coupler",
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
      "expr": "10",
      "unit": "mm",
      "model": false
    },
    {
      "name": "t",
      "expr": "1",
      "unit": "mm",
      "model": false
    },
    {
      "name": "r",
      "expr": "3.5",
      "unit": "mm",
      "model": false
    },
    {
      "name": "d",
      "expr": "7",
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
      "length": "2 * l + a",
      "width": "a",
      "height": "b",
      "operation": "new_body"
    },
    {
      "id": 2,
      "name": "Cylinder1",
      "suppressed": false,
      "param_names": [
        "d4",
        "d5"
      ],
      "participants": [
        "guide"
      ],
      "type": "cylinder",
      "base": [
        "l + a / 2 - d",
        "a / 2 - d",
        "3 * b / 4"
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r",
      "height": "t + b / 2",
      "operation": "join"
    },
    {
      "id": 3,
      "name": "Cylinder2",
      "suppressed": false,
      "param_names": [
        "d6",
        "d7"
      ],
      "participants": [
        "guide"
      ],
      "type": "cylinder",
      "base": [
        "l + a / 2 + d",
        "a / 2 + d",
        "3 * b / 4"
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r",
      "height": "t + b / 2",
      "operation": "join"
    },
    {
      "id": 4,
      "name": "Box2",
      "suppressed": false,
      "param_names": [
        "d8",
        "d9",
        "d10"
      ],
      "participants": [
        "guide"
      ],
      "type": "box",
      "corner": [
        "l",
        "-l",
        "b + t"
      ],
      "length": "a",
      "width": "2 * l + a",
      "height": "b",
      "operation": "join"
    }
  ],
  "next_id": 5,
  "next_component_color_index": 0
}