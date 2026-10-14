import { SearchField } from "~/components/search-field";
import { SegmentedControl } from "~/components/segmented-control";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";

export function StorageFilters({
  query,
  setQuery,
  category,
  setCategory,
  age,
  setAge,
  grouped,
  setGrouped,
  setPage,
}: {
  query: string;
  setQuery: (value: string) => void;
  category: string;
  setCategory: (value: string) => void;
  age: string;
  setAge: (value: string) => void;
  grouped: boolean;
  setGrouped: (value: boolean) => void;
  setPage: (page: number) => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <SearchField
        value={query}
        onChange={(value) => {
          setQuery(value);
          setPage(0);
        }}
        placeholder="Resource, project or database"
        label="Search Docker storage resources"
      />
      <Select
        value={category}
        onValueChange={(value) => {
          setCategory(value);
          setPage(0);
        }}
      >
        <SelectTrigger aria-label="Storage review filter" size="sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">All resources</SelectItem>
          <SelectItem value="container">Stopped containers</SelectItem>
          <SelectItem value="image">Unused images</SelectItem>
          <SelectItem value="volume">Unreferenced volumes</SelectItem>
          <SelectItem value="build_cache">Old build cache</SelectItem>
        </SelectContent>
      </Select>
      <Select
        value={age}
        onValueChange={(value) => {
          setAge(value);
          setPage(0);
        }}
      >
        <SelectTrigger aria-label="Stopped or last-used age" size="sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="1">Older than 1 day</SelectItem>
          <SelectItem value="7">Older than 7 days</SelectItem>
          <SelectItem value="30">Older than 30 days</SelectItem>
          <SelectItem value="all">All ages, including unknown</SelectItem>
          <SelectItem value="unknown">Unknown ages</SelectItem>
        </SelectContent>
      </Select>
      <SegmentedControl
        options={[
          { value: "flat", label: "Flat" },
          { value: "grouped", label: "By project" },
        ]}
        value={grouped ? "grouped" : "flat"}
        onChange={(value) => setGrouped(value === "grouped")}
        ariaLabel="Storage grouping"
      />
    </div>
  );
}
