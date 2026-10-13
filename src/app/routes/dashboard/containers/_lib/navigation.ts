/** Preserve native Link behavior for modified clicks and other targets. */
export function isContainerDetailClick(event: {
  defaultPrevented: boolean;
  button: number;
  metaKey: boolean;
  altKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  currentTarget: { target: string };
}): boolean {
  return (
    !event.defaultPrevented &&
    event.button === 0 &&
    !(event.metaKey || event.altKey || event.ctrlKey || event.shiftKey) &&
    (!event.currentTarget.target || event.currentTarget.target === "_self")
  );
}

/** Read scroll offsets when navigating, since scrolling need not rerender React. */
export function containerReturnState(
  location: { pathname: string; search: string },
  table = document.querySelector<HTMLElement>("[data-container-table-scroll]")
) {
  return {
    back: `${location.pathname}${location.search}`,
    scroll: document.querySelector("main")?.scrollTop ?? 0,
    tableScroll: table?.scrollTop ?? 0,
  };
}
