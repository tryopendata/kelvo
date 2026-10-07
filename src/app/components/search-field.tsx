import { SearchIcon, XIcon } from "lucide-react";
import styles from "./search-field.module.css";

/**
 * A filter field: the Processes header (name or PID) and the Power page's
 * energy table (app, process or PID). Esc clears it.
 */
export function SearchField({
  value,
  onChange,
  placeholder = "Name or PID",
  label = "Search processes by name or PID",
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  /** The field's accessible name. */
  label?: string;
}) {
  return (
    <div className="relative flex h-7 w-56 items-center">
      <SearchIcon
        aria-hidden
        className="pointer-events-none absolute left-2 size-3.5 text-muted-foreground"
      />
      <input
        type="search"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && value) {
            e.preventDefault();
            onChange("");
          }
        }}
        placeholder={placeholder}
        aria-label={label}
        spellCheck={false}
        autoComplete="off"
        className={`${styles.input} h-full w-full rounded-control border border-border bg-btn pr-7 pl-7 text-[12px] text-foreground outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring`}
      />
      {value && (
        <button
          type="button"
          onClick={() => onChange("")}
          aria-label="Clear search"
          className="absolute right-1.5 grid size-4 place-items-center rounded-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          <XIcon aria-hidden className="size-3" />
        </button>
      )}
    </div>
  );
}
