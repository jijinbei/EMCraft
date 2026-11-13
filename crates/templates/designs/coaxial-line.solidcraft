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
  "name": "Coaxial line",
  "units": "mm",
  "params": [
    {
      "name": "r_outer",
      "expr": "3.5",
      "unit": "mm",
      "model": false
    },
    {
      "name": "r_inner",
      "expr": "1",
      "unit": "mm",
      "model": false
    },
    {
      "name": "l",
      "expr": "20",
      "unit": "mm",
      "model": false
    }
  ],
  "features": [
    {
      "id": 1,
      "name": "Cylinder1",
      "suppressed": false,
      "body_names": [
        "line"
      ],
      "param_names": [
        "d1",
        "d2"
      ],
      "type": "cylinder",
      "base": [
        0.0,
        0.0,
        0.0
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r_outer",
      "height": "l",
      "operation": "new_body"
    },
    {
      "id": 2,
      "name": "Cylinder2",
      "suppressed": false,
      "body_names": [
        "inner"
      ],
      "param_names": [
        "d3",
        "d4"
      ],
      "type": "cylinder",
      "base": [
        0.0,
        0.0,
        0.0
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r_inner",
      "height": "l",
      "operation": "new_body"
    }
  ],
  "next_id": 3,
  "next_component_color_index": 0
}