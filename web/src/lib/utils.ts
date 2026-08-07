import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

/** Merge class lists, letting later Tailwind utilities win over earlier ones. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
