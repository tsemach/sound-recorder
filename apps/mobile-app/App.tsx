import React from "react"
import { SafeAreaView, StatusBar, StyleSheet } from "react-native"

import { MainScreen } from "./src/components/MainScreen"

function App(): React.JSX.Element {
  return (
    <SafeAreaView style={styles.safeArea}>
      <StatusBar barStyle="dark-content" />
      <MainScreen />
    </SafeAreaView>
  )
}

const styles = StyleSheet.create({
  safeArea: { flex: 1, backgroundColor: "#ffffff" },
})

export default App
