import React, { useState } from "react"
import { Alert, Pressable, StyleSheet, Text, View } from "react-native"

import type { AudioCapture } from "../capture/types"
import { useRecordingState } from "../hooks/useRecordingState"
import { formatDuration } from "../lib/format"

type MainScreenProps = {
  capture?: AudioCapture
}

export function MainScreen({ capture }: MainScreenProps = {}) {
  const {
    state,
    elapsedMs,
    level,
    sources,
    error,
    startRecording,
    pauseRecording,
    resumeRecording,
    stopRecording,
    cancelRecording,
  } = useRecordingState(capture)

  const [selectedSourceId, setSelectedSourceId] = useState<string | null>(null)

  const canStart =
    state.state === "Idle" ||
    state.state === "Saved" ||
    (state.state === "Error" && state.recoverable)
  const isRecording = state.state === "Recording"
  const isPaused = state.state === "Paused"
  const isActive = isRecording || isPaused
  const effectiveSourceId = selectedSourceId ?? sources[0]?.id ?? null

  function handleCancel() {
    Alert.alert("Discard this recording?", undefined, [
      { text: "Keep Recording", style: "cancel" },
      {
        text: "Discard",
        style: "destructive",
        onPress: () => void cancelRecording(),
      },
    ])
  }

  return (
    <View style={styles.container}>
      <Text style={styles.title}>Sound Recorder</Text>

      {error && (
        <View style={styles.errorBanner}>
          <Text style={styles.errorText}>{error}</Text>
        </View>
      )}

      {canStart && sources.length > 0 && (
        <View style={styles.sourceRow}>
          {sources.map((source) => (
            <Pressable
              key={source.id}
              onPress={() => setSelectedSourceId(source.id)}
              style={[
                styles.sourceChip,
                source.id === effectiveSourceId && styles.sourceChipSelected,
              ]}
            >
              <Text>{source.name}</Text>
            </Pressable>
          ))}
        </View>
      )}

      <Text style={styles.timer}>{formatDuration(isActive ? elapsedMs : 0)}</Text>

      {isActive && (
        <View style={styles.levelTrack}>
          <View
            style={[styles.levelFill, { width: `${Math.min(level, 1) * 100}%` }]}
          />
        </View>
      )}

      {state.state === "Saving" && <Text style={styles.saving}>Saving…</Text>}

      {state.state === "Saved" && (
        <Text style={styles.saved}>Saved · {state.filePath}</Text>
      )}

      <View style={styles.buttonRow}>
        {canStart && sources.length > 0 && effectiveSourceId && (
          <Pressable
            style={styles.button}
            onPress={() => void startRecording(effectiveSourceId)}
          >
            <Text style={styles.buttonText}>Record</Text>
          </Pressable>
        )}
        {isRecording && (
          <Pressable style={styles.button} onPress={() => pauseRecording()}>
            <Text style={styles.buttonText}>Pause</Text>
          </Pressable>
        )}
        {isPaused && (
          <Pressable style={styles.button} onPress={() => resumeRecording()}>
            <Text style={styles.buttonText}>Resume</Text>
          </Pressable>
        )}
        {isActive && (
          <Pressable style={styles.button} onPress={() => void stopRecording()}>
            <Text style={styles.buttonText}>Stop</Text>
          </Pressable>
        )}
        {isActive && (
          <Pressable style={styles.button} onPress={handleCancel}>
            <Text style={styles.buttonText}>Cancel</Text>
          </Pressable>
        )}
      </View>
    </View>
  )
}

const styles = StyleSheet.create({
  container: { flex: 1, padding: 24, gap: 16 },
  title: { fontSize: 20, fontWeight: "600" },
  errorBanner: {
    borderWidth: 1,
    borderColor: "#dc2626",
    borderRadius: 6,
    padding: 8,
  },
  errorText: { color: "#dc2626" },
  sourceRow: { flexDirection: "row", gap: 8, flexWrap: "wrap" },
  sourceChip: {
    borderWidth: 1,
    borderColor: "#999999",
    borderRadius: 16,
    paddingVertical: 6,
    paddingHorizontal: 12,
  },
  sourceChipSelected: { borderColor: "#2563eb", backgroundColor: "#dbeafe" },
  timer: { fontSize: 32, fontVariant: ["tabular-nums"] },
  levelTrack: {
    height: 8,
    borderRadius: 4,
    backgroundColor: "#e5e7eb",
    overflow: "hidden",
  },
  levelFill: { height: 8, backgroundColor: "#2563eb" },
  saving: { color: "#6b7280" },
  saved: { color: "#16a34a" },
  buttonRow: { flexDirection: "row", gap: 8 },
  button: {
    backgroundColor: "#2563eb",
    borderRadius: 6,
    paddingVertical: 10,
    paddingHorizontal: 16,
  },
  buttonText: { color: "#ffffff", fontWeight: "600" },
})
