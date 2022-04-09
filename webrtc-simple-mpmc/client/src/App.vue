<template>
	<h1>Simple multiple producers and multiple consumers</h1>
	<div>
		<button type="button" @click="start" :disabled="startButtonDisabled">start</button>
	</div>
	<div v-if="isStarted">
		<h2 class="publisher">My Video</h2>
		<video :src-object.prop.camel="srcObject" autoplay controls></video>
		<div>
			message: <input type="text" v-model="message" class="message-input"/>
			<button type="button" @click="sendMessage" :disabled="!message">send!</button>
		</div>
	</div>
	<div v-if="isStarted">
		<h2 class="subscriber">Watching ({{ videos.length }})</h2>
		<template v-for="video in videos" :key="video.id">
			<h3>{{ video.id }}</h3>
			<video :src-object.prop.camel="video.srcObject" autoplay></video>
			<div>
				<button type="button" @click="toggleMute(video.id)">
					{{ video.muteButtonName }}
				</button>
			</div>
			<div v-if="!!video.message">
				Message from the peer: {{ video.message }}
			</div>
		</template>
	</div>
</template>

<script lang="ts">
import { defineComponent, reactive } from 'vue';

enum AppState {
	Init,
	Started
}

interface DataType {
	srcObject: MediaStream | null,
	message: string,
	videos: Array<VideoWindow>,
	state: AppState
}

interface VideoWindow {
	id: string,
	srcObject: MediaStream | null,
	message: string,
	muteButtonName: 'mute' | 'unmute'
}

interface VideoHandle {
	videoWindow: VideoWindow,
	audio: HTMLAudioElement
}

enum SubscriberMessageType {
	Offer = 'Offer',
	Answer = 'Answer',
	Start = 'Start',
	Prepare = 'Prepare',
	IceCandidate = 'IceCandidate',
}

interface SubscriberMessage {
	msg_type: SubscriberMessageType,
	message: string
}

interface DataMessage {
	from: string,
	message: string
}

class ConnectionHandler {

	private socket: WebSocket | undefined;
	private pc: RTCPeerConnection | undefined;
	private dc: RTCDataChannel | undefined;
	private videoHandles: Map<string, VideoHandle>;

	constructor() {
		this.videoHandles = new Map();
	}
	
	async init(data: DataType): Promise<void> {

		const pc = await this.initRTCPeerConnection(data);

		const isHttps = location.protocol.startsWith('https:');
		const scheme = isHttps ? 'wss:' : 'ws:';
		this.socket = new WebSocket(`${scheme}//${location.host}/ws-app/subscribe`);

		pc.addEventListener('icecandidate', (event: RTCPeerConnectionIceEvent) => {
			this.sendMessage(JSON.stringify({
				msg_type: SubscriberMessageType.IceCandidate,
				message: JSON.stringify(event.candidate)
			}));
		});
		pc.addEventListener('datachannel', (event: RTCDataChannelEvent) => {

			const dataChannel = event.channel;
			this.dc = dataChannel;
			const label = dataChannel.label;

			dataChannel.onopen = () => console.debug('Data channel opened', label);
			dataChannel.onclose = () => console.debug('Data channel closed', label);

			dataChannel.onmessage = (event: MessageEvent) => {
				const msg = JSON.parse(event.data) as DataMessage;
				const videoId = 'sfu-stream-' + msg.from;
				const vh = this.videoHandles.get(videoId);
				if (vh) {
					vh.videoWindow.message = msg.message;
				}
			};

		});

		this.socket.addEventListener('open', () => {
			this.sendMessage(JSON.stringify({
				msg_type: SubscriberMessageType.Prepare,
				message: ''
			} as SubscriberMessage));
		});
		this.socket.addEventListener('message', async (event: MessageEvent) => {

			const message = JSON.parse(event.data) as SubscriberMessage;

			switch (message.msg_type) {
			case SubscriberMessageType.Offer: {

				if (!message.message) {
					console.error('Invalid message format', message);
					break;
				}

				const offer = JSON.parse(message.message);
				console.debug('---------------------- offer -----------------------------');
				console.debug(offer.sdp);
				console.debug('---------------------- offer -----------------------------');
				await pc.setRemoteDescription(offer);
				await pc.setLocalDescription(await pc.createAnswer());
				// await this.gatherIceCandidate(pc);
				const answer = pc.localDescription;

				if (!answer) {
					console.error('Answer is null');
					break;
				}
				console.debug('---------------------- answer -----------------------------');
				console.debug(answer.sdp);
				console.debug('---------------------- answer -----------------------------');

				this.sendMessage(JSON.stringify({
					msg_type: SubscriberMessageType.Answer,
					message: JSON.stringify(answer)
				}));
				break;
			}
			case SubscriberMessageType.IceCandidate: {
				const iceCandidate = JSON.parse(message.message);
				console.debug('Receive ICE candidate: ', iceCandidate);
				pc.addIceCandidate(iceCandidate);
				break;
			}
			default:
				break;
			}
		});
	}

	sendData(data: string): void {
		if (!this.dc) {
			return;
		}
		this.dc.send(data);
	}

	getVideoHandle(videoId: string): VideoHandle | undefined {
		return this.videoHandles.get(videoId);
	}

	private async initRTCPeerConnection(data: DataType): Promise<RTCPeerConnection> {

		const pc = await this.newRTCPeerConnection();

		pc.ontrack = (event: RTCTrackEvent) => {
			const mediaStream = event.streams[0];
			const videoId = mediaStream.id;

			console.debug('on_track', event.track);

			if (videoId.startsWith('sfu-stream-') && event.track.kind === 'video') {

				// https://stackoverflow.com/questions/34990672/control-volume-gain-for-video-audio-stream-in-firefox
				const audioTrack = mediaStream.getAudioTracks()[0];
				const audio = new Audio();
				audio.srcObject = new MediaStream([ audioTrack ]);
				audio.onloadedmetadata = () => {
					audio.play();
				};

				const videoStream  = new MediaStream([ mediaStream.getVideoTracks()[0] ]);

				const videoWindow: VideoWindow = reactive({
					id: videoId,
					srcObject: videoStream,
					message: '',
					muteButtonName: 'mute'
				});

				const videoHandle: VideoHandle = {
					videoWindow,
					audio
				};

				this.videoHandles.set(videoId, videoHandle);

				data.videos.push(videoWindow);

				event.track.onmute = () => {
					console.debug(`mute ${videoId}`);
					for (let i = 0; data.videos.length; i++) {
						const video = data.videos[i];
						if (!video) {
							continue;
						}
						if (videoId === video.id) {
							console.debug(`Remove the video whose index is ${i}`);
							data.videos.splice(i, 1);
							break;
						}
					}
				};
			}
		};
		const stream = await navigator.mediaDevices.getUserMedia({
			video: true,
			audio: true
		});

		data.srcObject = stream;
		stream.getTracks()
			.forEach(track => pc.addTrack(track, stream));

		return pc;
	}


	// 
	// Vanilla ICE
	// References
	// https://github.com/aiortc/aiortc/blob/main/examples/webcam/client.js
	// 
	/*
	private async gatherIceCandidate(pc: RTCPeerConnection): Promise<void> {
		return new Promise(resolve => {

			if (pc.iceGatheringState === 'complete') {
				resolve();
			} else {
				const checkState = () => {
					if (pc.iceGatheringState === 'complete') {
						pc.removeEventListener('icegatheringstatechange', checkState);
						resolve();
					}
				};
				pc.addEventListener('icegatheringstatechange', checkState);
			}

		});
	}*/

	private async newRTCPeerConnection(): Promise<RTCPeerConnection> {

		let iceServers = await fetch('/app/ice-servers').then(res => res.json());
		console.debug(iceServers);
		return new RTCPeerConnection({
			iceServers: iceServers,
			// iceTransportPolicy: 'relay'
		});
	}


	private sendMessage(text: string): void {
		if (!this.socket) {
			console.error('Socket is null');
			return;
		}
		this.socket.send(text);
	}
}

const App = defineComponent({
	setup() {
		// https://logaretm.com/blog/vue-composition-api-non-reactive-objects/
		const connectionHandler = new ConnectionHandler();
		return {
			connectionHandler
		};
	},
	data(): DataType {
		return {
			srcObject: null,
			message: '',
			videos: [],
			state: AppState.Init
		};
	},
	methods: {
		start(): void {
			this.state = AppState.Started;
			this.connectionHandler.init(this);
		},
		sendMessage(): void {
			this.connectionHandler.sendData(this.message);
		},
		toggleMute(videoId: string): void {
			const vh = this.connectionHandler.getVideoHandle(videoId);
			if (vh) {
				const muted = vh.audio.muted;
				const btn = muted ? 'mute' : 'unmute';
				vh.videoWindow.muteButtonName = btn;
				vh.audio.muted = !muted;
			}
		}
	},
	mounted() {
		setTimeout(() => {
			this.start();
		}, 1000);
	},
	computed: {
		startButtonDisabled(): boolean {
			const data = this as DataType;
			return data.state != AppState.Init;
		},
		isStarted(): boolean {
			const data = this as DataType;
			return data.state === AppState.Started;
		}
	}
});
export default App;
</script>

<style scoped>
.publisher {
	color: rgb(21, 9, 77);
}
.subscriber {
	color: rgb(90, 6, 17);
}
.message-input {
	width: 360px;
}
</style>