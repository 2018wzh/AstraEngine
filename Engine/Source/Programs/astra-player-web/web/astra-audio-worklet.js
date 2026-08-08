class AstraAudioProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.channels = options.processorOptions.channels;
    this.capacity = options.processorOptions.capacityFrames;
    this.queue = [];
    this.queuedFrames = 0;
    this.underflowCount = 0;
    this.refillRequested = false;
    this.port.onmessage = ({ data }) => {
      if (data.type === "packet") {
        const frames = data.samples.length / this.channels;
        if (this.queuedFrames + frames > this.capacity) {
          this.port.postMessage({ type: "overflow", sequence: data.sequence });
          return;
        }
        this.queue.push({ samples: data.samples, offset: 0, sequence: data.sequence });
        this.queuedFrames += frames;
        this.refillRequested = false;
      } else if (data.type === "empty") {
        this.refillRequested = false;
      }
    };
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    const frames = output[0]?.length ?? 0;
    let underflow = false;
    for (let frame = 0; frame < frames; frame += 1) {
      const packet = this.queue[0];
      if (!packet) {
        underflow = true;
      }
      for (let channel = 0; channel < output.length; channel += 1) {
        const sample = packet ? packet.samples[packet.offset + channel] ?? 0 : 0;
        output[channel][frame] = sample;
      }
      if (packet) {
        packet.offset += this.channels;
        this.queuedFrames -= 1;
        if (packet.offset >= packet.samples.length) {
          this.queue.shift();
        }
      }
    }
    if (underflow) {
      this.underflowCount += 1;
    }
    if (this.queuedFrames <= this.capacity / 2 && !this.refillRequested) {
      this.refillRequested = true;
      this.port.postMessage({ type: "refill" });
    }
    return true;
  }
}

registerProcessor("astra-audio-output", AstraAudioProcessor);
