export const workshopKeys = {
  all: ["workshop"] as const,
  projects: () => [...workshopKeys.all, "projects"] as const,
  project: (path: string) => [...workshopKeys.projects(), path] as const,
  validation: (path: string) => [...workshopKeys.project(path), "validation"] as const,
  thumbnail: (path: string, thumbnailPath?: string) =>
    [...workshopKeys.project(path), "thumbnail", thumbnailPath] as const,
};
