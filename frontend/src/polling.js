export function mergeServerFilesWithOptimistic(prevFiles, serverFiles) {
  const serverIds = new Set(serverFiles.map((f) => f.id));
  const stillUploadingOptimistic = prevFiles.filter(
    (f) => f.status === 'uploading' && !serverIds.has(f.id),
  );
  return [...stillUploadingOptimistic, ...serverFiles];
}

export function hasActiveJobs(files) {
  return files.some((f) => f && (f.status === 'uploaded' || f.status === 'processing'));
}
