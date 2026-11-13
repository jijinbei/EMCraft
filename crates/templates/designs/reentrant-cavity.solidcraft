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
  "name": "Re-entrant cavity",
  "units": "mm",
  "params": [
    {
      "name": "r",
      "expr": "15",
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
      "name": "r_post",
      "expr": "3",
      "unit": "mm",
      "model": false
    },
    {
      "name": "gap",
      "expr": "2",
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
        "cavity"
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
      "radius": "r",
      "height": "l",
      "operation": "new_body"
    },
    {
      "id": 2,
      "name": "Cylinder2",
      "suppressed": false,
      "body_names": [
        "post"
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
      "radius": "r_post",
      "height": "l - gap",
      "operation": "new_body"
    }
  ],
  "next_id": 3,
  "next_component_color_index": 0
}