import { useEffect, useState } from "react";
import { useDebounceCallback } from "usehooks-ts";

export function useDebouncedSlider(
  settingsValue: number,
  onSave: (value: number) => void,
  delay = 150,
) {
  const [localValue, setLocalValue] = useState(settingsValue);

  useEffect(() => {
    setLocalValue(settingsValue);
  }, [settingsValue]);

  const debouncedSave = useDebounceCallback(onSave, delay);

  function handleChange(value: number) {
    setLocalValue(value);
    debouncedSave(value);
  }

  return [localValue, handleChange] as const;
}
