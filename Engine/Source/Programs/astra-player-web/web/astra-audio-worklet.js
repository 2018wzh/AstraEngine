class AstraAudioProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.channels = options.processorOptions.channels;
    this.capacity = options.processorOptions.capacityFrames;
    this.queue = [];
    this.queuedFrames = 0;
    this.sampleCount = 0;
    this.sumSquares = 0;
    this.peak = 0;
    this.underflowCount = 0;
    this.callbackCount = 0;
    this.port.onmessage = ({ data }) => {
      if (data.type === "packet") {
        const frames = data.samples.length / this.channels;
        if (this.queuedFrames + frames > this.capacity) {
          this.port.postMessage({ type: "overflow", sequence: data.sequence });
          return;
        }
        this.queue.push({ samples: data.samples, offset: 0, sequence: data.sequence });
        this.queuedFrames += frames;
      } else if (data.type === "meter") {
        const rms = this.sampleCount ? Math.sqrt(this.sumSquares / this.sampleCount) : 0;
        this.port.postMessage({
          type: "meter",
          sampleCount: this.sampleCount,
          peak: this.peak,
          rms,
          queuedFrames: this.queuedFrames,
          underflowCount: this.underflowCount,
          callbackCount: this.callbackCount,
        });
      }
    };
  }

  process(_inputs, outputs) {
    this.callbackCount += 1;
    const output = outputs[0];
    const frames = output[0]?.length ?? 0;
    for (let frame = 0; frame < frames; frame += 1) {
      const packet = this.queue[0];
      if (!packet) {
        this.underflowCount += 1;
      }
      for (let channel = 0; channel < output.length; channel += 1) {
        const sample = packet ? packet.samples[packet.offset + channel] ?? 0 : 0;
        output[channel][frame] = sample;
        if (packet) {
          this.peak = Math.max(this.peak, Math.abs(sample));
          this.sumSquares += sample * sample;
          this.sampleCount += 1;
        }
      }
      if (packet) {
        packet.offset += this.channels;
        this.queuedFrames -= 1;
        if (packet.offset >= packet.samples.length) {
          this.queue.shift();
          this.port.postMessage({ type: "consumed", sequence: packet.sequence });
        }
      }
    }
    return true;
  }
}

registerProcessor("astra-audio-output", AstraAudioProcessor);
