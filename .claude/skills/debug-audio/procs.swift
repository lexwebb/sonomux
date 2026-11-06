import CoreAudio
import Foundation
// Lists HAL audio processes that run input or output: pid, input?, output?, bundle.
func get<T>(_ id: AudioObjectID, _ sel: AudioObjectPropertySelector, _ v: inout T) {
  var a = AudioObjectPropertyAddress(mSelector: sel, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
  var s = UInt32(MemoryLayout<T>.size); AudioObjectGetPropertyData(id, &a, 0, nil, &s, &v)
}
var a = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyProcessObjectList, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
var size: UInt32 = 0
AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &a, 0, nil, &size)
var ids = [AudioObjectID](repeating: 0, count: Int(size) / 4)
AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &a, 0, nil, &size, &ids)
for id in ids {
  var pid: pid_t = 0, inp: UInt32 = 0, out: UInt32 = 0
  get(id, kAudioProcessPropertyPID, &pid); get(id, kAudioProcessPropertyIsRunningInput, &inp); get(id, kAudioProcessPropertyIsRunningOutput, &out)
  var cf: CFString = "" as CFString; get(id, kAudioProcessPropertyBundleID, &cf)
  if inp != 0 || out != 0 { print(pid, "in=\(inp)", "out=\(out)", cf) }
}
