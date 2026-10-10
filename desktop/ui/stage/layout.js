// Where the dot and the chat panel go on the overlay: plain geometry in its pixels, where the
// screen is { width, height } and a box is { x, y, width, height }.

/** [box] moved just enough to fit on [screen], [margin] from its edges. */
export function keepOn(screen, box, margin) {
  return {
    ...box,
    x: Math.max(margin, Math.min(box.x, screen.width - box.width - margin)),
    y: Math.max(margin, Math.min(box.y, screen.height - box.height - margin)),
  };
}

/** The dot: where it was left ([saved], else in the bottom-right corner, [edge] from it), on screen. */
export function placeDot(screen, saved, size, edge, margin) {
  const at = saved ?? { x: screen.width - size - edge, y: screen.height - size - edge };
  return keepOn(screen, { x: at.x, y: at.y, width: size, height: size }, margin);
}

/**
 * The panel, from the dot toward the middle of the screen, [gap] away: above it when the dot is in
 * the bottom half, and lined up with its right edge when in the right half. [up] and [fromRight]
 * say which, for the corner it grows from.
 */
export function placePanel(screen, dot, size, gap, margin) {
  const up = dot.y + dot.height / 2 > screen.height / 2;
  const fromRight = dot.x + dot.width / 2 > screen.width / 2;
  const box = keepOn(screen, {
    x: fromRight ? dot.x + dot.width - size.width : dot.x,
    y: up ? dot.y - gap - size.height : dot.y + dot.height + gap,
    ...size,
  }, margin);
  return { ...box, up, fromRight };
}
