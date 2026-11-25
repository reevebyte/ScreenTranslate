import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export type WithoutChild<T> = T extends unknown ? Omit<T, 'child'> : never;
export type WithoutChildren<T> = T extends unknown ? Omit<T, 'children'> : never;
export type WithoutChildrenOrChild<T> = T extends unknown ? Omit<T, 'children' | 'child'> : never;
export type WithElementRef<T, U extends HTMLElement = HTMLElement> = T & { ref?: U | null };

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
