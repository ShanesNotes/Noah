const WebSocket = require('ws');
const wss = new WebSocket.Server({ port: 8080 });

const patients = [
    {
        patientId: "Seymore Butts",
        vitals: { HR: 90, SpO2: 95, BP: "120/80", MAP: 93, RR: 18, Temp: 38.2, TidalVolume: 420 },
        hourlyProfile: Array.from({ length: 24 }, (_, h) => ({
            HR: 90 + h * 2 - (h > 12 ? (h - 12) * 3 : 0),
            SpO2: 95 - h * 0.5,
            MAP: 93 - h * 1.5,
            RR: 18 + h * 0.5,
            Temp: 38.2 + h * 0.05 - (h > 18 ? 0.5 : 0),
        })),
    },
    // Add other patients similarly...
];

const SECONDS_PER_DAY = 86400;
function getSimulatedHour() {
    return Math.floor((Math.floor(Date.now() / 1000) % SECONDS_PER_DAY) / 3600);
}

function updatePatientVitals(patient) {
    const hour = getSimulatedHour();
    const base = patient.hourlyProfile[hour];
    patient.vitals.HR = base.HR + (Math.random() - 0.5) * 2;
    patient.vitals.SpO2 = base.SpO2 + (Math.random() - 0.5) * 1;
    patient.vitals.MAP = base.MAP + (Math.random() - 0.5) * 2;
    patient.vitals.RR = base.RR + (Math.random() - 0.5) * 1;
    patient.vitals.Temp = base.Temp + (Math.random() - 0.5) * 0.1;
    patient.vitals.BP = `${Math.round(patient.vitals.MAP * 1.5)}/${Math.round(patient.vitals.MAP * 0.8)}`;
    return patient;
}

wss.on('connection', ws => {
    console.log('Noah connected to Phillips API simulation');
    const interval = setInterval(() => {
        const updatedPatients = patients.map(updatePatientVitals);
        ws.send(JSON.stringify({ patients: updatedPatients.map(p => ({ patientId: p.patientId, vitals: p.vitals })) }));
    }, 1000);
    ws.on('close', () => {
        clearInterval(interval);
        console.log('Noah disconnected');
    });
});

console.log('Phillips API Simulation running on ws://localhost:8080/vitals');