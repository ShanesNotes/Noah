document.addEventListener('DOMContentLoaded', () => {
    // DOM Elements
    const patientList = document.getElementById('patient-list');
    const patientDetail = document.getElementById('patient-detail');
    const patientSearch = document.getElementById('patient-search');
    const vitalsContainer = document.getElementById('vitals-container');
    const notificationContainer = document.getElementById('notification-container');
    const aiModelSelect = document.getElementById('ai-model');
    
    // State
    let patients = [];
    let selectedPatientId = null;
    let vitalsData = {};
    
    // Initialize WebSocket connection
    const socket = new WebSocket('ws://localhost:8080');
    
    socket.onopen = () => {
        console.log('Connected to vitals server');
        showNotification('Connection established', 'Successfully connected to vitals monitoring server', 'info');
    };
    
    socket.onclose = () => {
        console.log('Disconnected from vitals server');
        showNotification('Connection lost', 'Lost connection to vitals monitoring server', 'warning');
    };
    
    socket.onerror = (error) => {
        console.error('WebSocket error:', error);
        showNotification('Connection error', 'Failed to connect to vitals monitoring server', 'critical');
    };
    
    socket.onmessage = (event) => {
        try {
            const data = JSON.parse(event.data);
            if (data.patients) {
                updatePatients(data.patients);
                updateVitalsDisplay();
            }
        } catch (error) {
            console.error('Error parsing WebSocket message:', error);
        }
    };
    
    // Functions
    function updatePatients(newPatients) {
        // Update our patients data
        patients = newPatients;
        vitalsData = {};
        
        // Store vitals data by patient ID
        patients.forEach(patient => {
            vitalsData[patient.patientId] = patient.vitals;
        });
        
        // If this is the first data, render the patient list
        if (patientList.children.length === 0) {
            renderPatientList();
        }
    }
    
    function renderPatientList() {
        patientList.innerHTML = '';
        
        patients.forEach(patient => {
            const li = document.createElement('li');
            li.className = 'patient-item';
            li.dataset.patientId = patient.patientId;
            
            if (selectedPatientId === patient.patientId) {
                li.classList.add('active');
            }
            
            li.innerHTML = `
                <div class="patient-name">${patient.patientId}</div>
                <div class="patient-mrn">MRN: ${patient.patientId.replace(/\s/g, '').toLowerCase()}</div>
            `;
            
            li.addEventListener('click', () => {
                document.querySelectorAll('.patient-item').forEach(item => {
                    item.classList.remove('active');
                });
                li.classList.add('active');
                selectedPatientId = patient.patientId;
                renderPatientDetail(patient);
                updateVitalsDisplay();
            });
            
            patientList.appendChild(li);
        });
    }
    
    function renderPatientDetail(patient) {
        patientDetail.innerHTML = `
            <div class="patient-header">
                <div class="patient-info">
                    <h3>${patient.patientId}</h3>
                    <div class="patient-meta">
                        <span>MRN: ${patient.patientId.replace(/\s/g, '').toLowerCase()}</span>
                        <span>DOB: 01/15/1975</span>
                        <span>Sex: M</span>
                    </div>
                </div>
                <div class="patient-actions">
                    <button class="btn btn-outline">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                            <path d="M12 5V19M5 12H19" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                        </svg>
                        Add Note
                    </button>
                    <button class="btn btn-primary">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                            <path d="M9 12H15M12 9V15M3 12C3 13.1819 3.23279 14.3522 3.68508 15.4442C4.13738 16.5361 4.80031 17.5282 5.63604 18.364C6.47177 19.1997 7.46392 19.8626 8.55585 20.3149C9.64778 20.7672 10.8181 21 12 21C13.1819 21 14.3522 20.7672 15.4442 20.3149C16.5361 19.8626 17.5282 19.1997 18.364 18.364C19.1997 17.5282 19.8626 16.5361 20.3149 15.4442C20.7672 14.3522 21 13.1819 21 12C21 10.8181 20.7672 9.64778 20.3149 8.55585C19.8626 7.46392 19.1997 6.47177 18.364 5.63604C17.5282 4.80031 16.5361 4.13738 15.4442 3.68508C14.3522 3.23279 13.1819 3 12 3C10.8181 3 9.64778 3.23279 8.55585 3.68508C7.46392 4.13738 6.47177 4.80031 5.63604 5.63604C4.80031 6.47177 4.13738 7.46392 3.68508 8.55585C3.23279 9.64778 3 10.8181 3 12Z" stroke="currentColor" stroke-width="2"/>
                        </svg>
                        Order Labs
                    </button>
                </div>
            </div>
            
            <div class="patient-tabs">
                <div class="tab active" data-tab="summary">Summary</div>
                <div class="tab" data-tab="labs">Labs</div>
                <div class="tab" data-tab="meds">Medications</div>
                <div class="tab" data-tab="notes">Notes</div>
            </div>
            
            <div class="tab-content active" data-tab="summary">
                <div class="vital-summary">
                    ${renderVitalsSummary(patient.patientId)}
                </div>
                
                <h4 class="section-title">Recent Activity</h4>
                <div class="timeline">
                    <div class="timeline-item">
                        <div class="timeline-icon">
                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                                <path d="M9 12L11 14L15 10M21 12C21 16.9706 16.9706 21 12 21C7.02944 21 3 16.9706 3 12C3 7.02944 7.02944 3 12 3C16.9706 3 21 7.02944 21 12Z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                            </svg>
                        </div>
                        <div class="timeline-content">
                            <p class="timeline-time">Today, 10:30 AM</p>
                            <p class="timeline-text">Medication administered: Acetaminophen 650mg PO</p>
                        </div>
                    </div>
                    <div class="timeline-item">
                        <div class="timeline-icon">
                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                                <path d="M9 5H7C5.89543 5 5 5.89543 5 7V19C5 20.1046 5.89543 21 7 21H17C18.1046 21 19 20.1046 19 19V7C19 5.89543 18.1046 5 17 5H15M9 5C9 6.10457 9.89543 7 11 7H13C14.1046 7 15 6.10457 15 5M9 5C9 3.89543 9.89543 3 11 3H13C14.1046 3 15 3.89543 15 5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                            </svg>
                        </div>
                        <div class="timeline-content">
                            <p class="timeline-time">Today, 9:15 AM</p>
                            <p class="timeline-text">Nursing note added by Jane Smith, RN</p>
                        </div>
                    </div>
                    <div class="timeline-item">
                        <div class="timeline-icon">
                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                                <path d="M7 11.5V14M12 9V14M17 6.5V14M5 17H19C20.1046 17 21 16.1046 21 15V7C21 5.89543 20.1046 5 19 5H5C3.89543 5 3 5.89543 3 7V15C3 16.1046 3.89543 17 5 17Z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                            </svg>
                        </div>
                        <div class="timeline-content">
                            <p class="timeline-time">Today, 8:00 AM</p>
                            <p class="timeline-text">Lab results: Complete Blood Count</p>
                        </div>
                    </div>
                </div>
            </div>
            
            <div class="tab-content" data-tab="labs">
                <div class="labs-list">
                    <div class="lab-item">
                        <div class="lab-header">
                            <div class="lab-name">Complete Blood Count</div>
                            <div class="lab-date">Today, 8:00 AM</div>
                        </div>
                        <div class="lab-results">
                            <div class="lab-result">
                                <span class="result-name">WBC</span>
                                <span class="result-value">7.2</span>
                                <span class="result-unit">10³/µL</span>
                                <span class="result-range">4.5-11.0</span>
                            </div>
                            <div class="lab-result">
                                <span class="result-name">RBC</span>
                                <span class="result-value">4.8</span>
                                <span class="result-unit">10⁶/µL</span>
                                <span class="result-range">4.5-5.9</span>
                            </div>
                            <div class="lab-result">
                                <span class="result-name">Hemoglobin</span>
                                <span class="result-value">14.2</span>
                                <span class="result-unit">g/dL</span>
                                <span class="result-range">13.5-17.5</span>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
            
            <div class="tab-content" data-tab="meds">
                <div class="meds-list">
                    <div class="med-item">
                        <div class="med-name">Acetaminophen</div>
                        <div class="med-details">650mg PO Q6H PRN for pain</div>
                        <div class="med-status">Last given: Today, 10:30 AM</div>
                    </div>
                    <div class="med-item">
                        <div class="med-name">Lisinopril</div>
                        <div class="med-details">10mg PO Daily</div>
                        <div class="med-status">Last given: Today, 9:00 AM</div>
                    </div>
                </div>
            </div>
            
            <div class="tab-content" data-tab="notes">
                <div class="notes-list">
                    <div class="note-item">
                        <div class="note-header">
                            <div class="note-title">Nursing Note</div>
                            <div class="note-author">Jane Smith, RN</div>
                            <div class="note-date">Today, 9:15 AM</div>
                        </div>
                        <div class="note-content">
                            Patient reports pain level of 4/10. Acetaminophen administered with good effect. Vital signs stable.
                        </div>
                    </div>
                </div>
            </div>
        `;
        
        // Add tab switching functionality
        document.querySelectorAll('.tab').forEach(tab => {
            tab.addEventListener('click', () => {
                const tabName = tab.dataset.tab;
                
                // Update active tab
                document.querySelectorAll('.tab').forEach(t => {
                    t.classList.remove('active');
                });
                tab.classList.add('active');
                
                // Update active content
                document.querySelectorAll('.tab-content').forEach(content => {
                    content.classList.remove('active');
                });
                document.querySelector(`.tab-content[data-tab="${tabName}"]`).classList.add('active');
            });
        });
    }
    
    function renderVitalsSummary(patientId) {
        const vitals = vitalsData[patientId];
        if (!vitals) return '<p>No vitals data available</p>';
        
        return `
            <div class="vitals-summary-grid">
                <div class="vital-summary-item">
                    <div class="vital-name">Heart Rate</div>
                    <div class="vital-value ${getVitalClass(vitals.heartRate, 60, 100, 50, 120)}">${vitals.heartRate} bpm</div>
                </div>
                <div class="vital-summary-item">
                    <div class="vital-name">Blood Pressure</div>
                    <div class="vital-value ${getVitalClass(vitals.systolic, 90, 140, 80, 160)}">${vitals.systolic}/${vitals.diastolic} mmHg</div>
                </div>
                <div class="vital-summary-item">
                    <div class="vital-name">Respiratory Rate</div>
                    <div class="vital-value ${getVitalClass(vitals.respiratoryRate, 12, 20, 8, 25)}">${vitals.respiratoryRate} bpm</div>
                </div>
                <div class="vital-summary-item">
                    <div class="vital-name">Oxygen Saturation</div>
                    <div class="vital-value ${getVitalClass(vitals.oxygenSaturation, 95, 100, 90, 100)}">${vitals.oxygenSaturation}%</div>
                </div>
                <div class="vital-summary-item">
                    <div class="vital-name">Temperature</div>
                    <div class="vital-value ${getVitalClass(vitals.temperature, 36.5, 37.5, 36, 38)}">${vitals.temperature}°C</div>
                </div>
            </div>
        `;
    }
    
    function updateVitalsDisplay() {
        if (!selectedPatientId) {
            vitalsContainer.innerHTML = '<p class="empty-message">Select a patient to view vitals</p>';
            return;
        }
        
        const vitals = vitalsData[selectedPatientId];
        if (!vitals) {
            vitalsContainer.innerHTML = '<p class="empty-message">No vitals data available for this patient</p>';
            return;
        }
        
        vitalsContainer.innerHTML = `
            <div class="vital-card">
                <div class="vital-name">Heart Rate</div>
                <div class="vital-value ${getVitalClass(vitals.heartRate, 60, 100, 50, 120)}">${vitals.heartRate} bpm</div>
                <div class="vital-trend">
                    <svg width="100" height="30" viewBox="0 0 100 30" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <path d="M0 15 L10 15 L15 5 L25 25 L35 15 L45 15 L50 5 L60 25 L70 15 L80 15 L85 5 L95 15 L100 15" stroke="currentColor" stroke-width="2" fill="none"/>
                    </svg>
                </div>
            </div>
            <div class="vital-card">
                <div class="vital-name">Blood Pressure</div>
                <div class="vital-value ${getVitalClass(vitals.systolic, 90, 140, 80, 160)}">${vitals.systolic}/${vitals.diastolic} mmHg</div>
                <div class="vital-trend">
                    <svg width="100" height="30" viewBox="0 0 100 30" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <path d="M0 20 L10 18 L20 15 L30 16 L40 14 L50 15 L60 10 L70 12 L80 8 L90 10 L100 5" stroke="currentColor" stroke-width="2" fill="none"/>
                    </svg>
                </div>
            </div>
            <div class="vital-card">
                <div class="vital-name">Respiratory Rate</div>
                <div class="vital-value ${getVitalClass(vitals.respiratoryRate, 12, 20, 8, 25)}">${vitals.respiratoryRate} bpm</div>
                <div class="vital-trend">
                    <svg width="100" height="30" viewBox="0 0 100 30" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <path d="M0 15 L10 14 L20 16 L30 15 L40 17 L50 14 L60 16 L70 15 L80 13 L90 15 L100 14" stroke="currentColor" stroke-width="2" fill="none"/>
                    </svg>
                </div>
            </div>
            <div class="vital-card">
                <div class="vital-name">Oxygen Saturation</div>
                <div class="vital-value ${getVitalClass(vitals.oxygenSaturation, 95, 100, 90, 100)}">${vitals.oxygenSaturation}%</div>
                <div class="vital-trend">
                    <svg width="100" height="30" viewBox="0 0 100 30" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <path d="M0 10 L10 8 L20 5 L30 7 L40 6 L50 4 L60 5 L70 3 L80 5 L90 4 L100 5" stroke="currentColor" stroke-width="2" fill="none"/>
                    </svg>
                </div>
            </div>
            <div class="vital-card">
                <div class="vital-name">Temperature</div>
                <div class="vital-value ${getVitalClass(vitals.temperature, 36.5, 37.5, 36, 38)}">${vitals.temperature}°C</div>
                <div class="vital-trend">
                    <svg width="100" height="30" viewBox="0 0 100 30" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <path d="M0 15 L10 16 L20 17 L30 15 L40 16 L50 18 L60 17 L70 19 L80 18 L90 20 L100 19" stroke="currentColor" stroke-width="2" fill="none"/>
                    </svg>
                </div>
            </div>
        `;
    }
    
    function getVitalClass(value, normalLow, normalHigh, criticalLow, criticalHigh) {
        if (value < criticalLow || value > criticalHigh) {
            return 'vital-critical';
        } else if (value < normalLow || value > normalHigh) {
            return 'vital-warning';
        } else {
            return 'vital-normal';
        }
    }
    
    function showNotification(title, message, type = 'info') {
        const notification = document.createElement('div');
        notification.className = `notification notification-${type}`;
        notification.innerHTML = `
            <div class="notification-header">
                <div class="notification-title">${title}</div>
                <button class="notification-close">×</button>
            </div>
            <div class="notification-message">${message}</div>
        `;
        
        notification.querySelector('.notification-close').addEventListener('click', () => {
            notification.remove();
        });
        
        notificationContainer.appendChild(notification);
        
        // Auto-remove after 5 seconds
        setTimeout(() => {
            notification.style.opacity = '0';
            notification.style.transform = 'translateX(100%)';
            setTimeout(() => {
                notification.remove();
            }, 300);
        }, 5000);
    }
    
    // Event listeners
    patientSearch.addEventListener('input', (e) => {
        const searchTerm = e.target.value.toLowerCase();
        
        document.querySelectorAll('.patient-item').forEach(item => {
            const patientName = item.querySelector('.patient-name').textContent.toLowerCase();
            const patientMRN = item.querySelector('.patient-mrn').textContent.toLowerCase();
            
            if (patientName.includes(searchTerm) || patientMRN.includes(searchTerm)) {
                item.style.display = 'block';
            } else {
                item.style.display = 'none';
            }
        });
    });
    
    aiModelSelect.addEventListener('change', (e) => {
        const selectedModel = e.target.value;
        console.log(`AI model changed to: ${selectedModel}`);
        showNotification('AI Model Changed', `Now using ${selectedModel} for analysis`, 'info');
        
        // Here you would typically send a message to the server to change the AI model
        if (socket.readyState === WebSocket.OPEN) {
            socket.send(JSON.stringify({
                type: 'changeModel',
                model: selectedModel
            }));
        }
    });
    
    // Initialize with mock data if needed
    if (!socket || socket.readyState !== WebSocket.OPEN) {
        const mockPatients = [
            {
                patientId: 'John Doe',
                vitals: {
                    heartRate: 72,
                    systolic: 120,
                    diastolic: 80,
                    respiratoryRate: 16,
                    oxygenSaturation: 98,
                    temperature: 37.0
                }
            },
            {
                patientId: 'Jane Smith',
                vitals: {
                    heartRate: 85,
                    systolic: 135,
                    diastolic: 85,
                    respiratoryRate: 18,
                    oxygenSaturation: 96,
                    temperature: 37.2
                }
            }
        ];
        
        updatePatients(mockPatients);
    }
}); 